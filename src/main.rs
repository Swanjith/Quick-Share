use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{Context, Result};
use base64::{engine::general_purpose, Engine as _};
use clap::{Parser, Subcommand};
use indicatif::{ProgressBar, ProgressStyle};
use local_ip_address::local_ip;
use rand::Rng;
use reqwest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use uuid::Uuid;

const CHUNK_SIZE: usize = 64 * 1024; // 64KB chunks
const PORT: u16 = 8080;
const DEFAULT_STREAMS: usize = 10; // Number of parallel streams

fn format_file_size(size: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = size as f64;
    let mut unit_index = 0;
    
    while size >= 1024.0 && unit_index < UNITS.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }
    
    if unit_index == 0 {
        format!("{} {}", size as u64, UNITS[unit_index])
    } else {
        format!("{:.1} {}", size, UNITS[unit_index])
    }
}

fn generate_key() -> [u8; 32] {
    let mut rng = rand::thread_rng();
    let mut key = [0u8; 32];
    rng.fill(&mut key);
    key
}

fn key_from_string(key_str: &str) -> Result<[u8; 32]> {
    let decoded = general_purpose::STANDARD
        .decode(key_str)
        .context("Failed to decode base64 key")?;
    
    if decoded.len() != 32 {
        return Err(anyhow::anyhow!("Key must be exactly 32 bytes"));
    }
    
    let mut key = [0u8; 32];
    key.copy_from_slice(&decoded);
    Ok(key)
}

fn encrypt_data(data: &[u8], cipher: &Aes256Gcm) -> Result<Vec<u8>> {
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(&nonce, data)
        .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;
    
    // Prepend nonce to ciphertext
    let mut result = nonce.to_vec();
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

fn decrypt_data(encrypted_data: &[u8], cipher: &Aes256Gcm) -> Result<Vec<u8>> {
    if encrypted_data.len() < 12 {
        return Err(anyhow::anyhow!("Invalid encrypted data length"));
    }
    
    let (nonce_bytes, ciphertext) = encrypted_data.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))
}

async fn get_external_ip() -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()?;
    
    // Try multiple IP detection services
    let services = [
        "https://api.ipify.org",
        "https://ifconfig.me/ip",
        "https://icanhazip.com",
        "https://checkip.amazonaws.com",
    ];
    
    for service in &services {
        if let Ok(response) = client.get(*service).send().await {
            if let Ok(ip) = response.text().await {
                let ip = ip.trim();
                if !ip.is_empty() && ip.parse::<std::net::IpAddr>().is_ok() {
                    return Ok(ip.to_string());
                }
            }
        }
    }
    
    Err(anyhow::anyhow!("Failed to detect external IP address"))
}

#[derive(Serialize, Deserialize, Debug)]
struct RelayMessage {
    message_type: String,
    session_id: String,
    data: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
struct RelaySession {
    session_id: String,
    sender_connected: bool,
    receiver_connected: bool,
}

#[derive(Parser)]
#[command(name = "fastshare")]
#[command(about = "Ultra-fast file sharing between devices on the same network")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Send a file to another device
    Send {
        /// Path to the file to send
        #[arg(value_name = "FILE")]
        file: PathBuf,
        /// Port to listen on (default: 8080)
        #[arg(short, long, default_value_t = PORT)]
        port: u16,
        /// Number of parallel streams (default: 10)
        #[arg(short, long, default_value_t = DEFAULT_STREAMS)]
        streams: usize,
        /// Enable encryption (generates a random key)
        #[arg(short, long)]
        encrypt: bool,
        /// Custom encryption key (base64 encoded, 32 bytes)
        #[arg(short, long)]
        key: Option<String>,
        /// Enable public network mode (shows external IP and port forwarding info)
        #[arg(long)]
        public: bool,
        /// Use relay server for NAT traversal
        #[arg(long)]
        relay: Option<String>,
    },
    /// Receive a file from another device
    Receive {
        /// IP address of the sender
        #[arg(value_name = "IP")]
        ip: String,
        /// Port to connect to (default: 8080)
        #[arg(short, long, default_value_t = PORT)]
        port: u16,
        /// Output directory (default: current directory)
        #[arg(short, long, default_value = ".")]
        output: PathBuf,
        /// Number of parallel streams (default: 10)
        #[arg(short, long, default_value_t = DEFAULT_STREAMS)]
        streams: usize,
        /// Encryption key (base64 encoded, 32 bytes) - required if sender uses encryption
        #[arg(short, long)]
        key: Option<String>,
        /// Use relay server for NAT traversal
        #[arg(long)]
        relay: Option<String>,
    },
    /// Run as a relay server for public network transfers
    Relay {
        /// Port to listen on (default: 8080)
        #[arg(short, long, default_value_t = PORT)]
        port: u16,
    },
}

#[derive(Serialize, Deserialize, Debug)]
struct FileInfo {
    name: String,
    size: u64,
    streams: usize,
    encrypted: bool,
}

#[derive(Serialize, Deserialize, Debug)]
struct LegacyFileInfo {
    name: String,
    size: u64,
}

#[derive(Serialize, Deserialize, Debug)]
struct ChunkInfo {
    stream_id: usize,
    offset: u64,
    size: usize,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Send { file, port, streams, encrypt, key, public, relay } => {
            send_file(file, port, streams, encrypt, key, public, relay).await
        }
        Commands::Receive { ip, port, output, streams, key, relay } => {
            receive_file(ip, port, output, streams, key, relay).await
        }
        Commands::Relay { port } => {
            run_relay_server(port).await
        }
    }
}

async fn send_file(file_path: PathBuf, port: u16, num_streams: usize, encrypt: bool, key: Option<String>, public: bool, relay: Option<String>) -> Result<()> {
    // Get file info
    let file_size = tokio::fs::metadata(&file_path)
        .await
        .context("Failed to get file metadata")?
        .len();
    
    let file_name = file_path
        .file_name()
        .context("Invalid file path")?
        .to_string_lossy()
        .to_string();

    // Setup encryption if requested
    let cipher = if encrypt || key.is_some() {
        let encryption_key = if let Some(key_str) = &key {
            key_from_string(key_str)?
        } else {
            generate_key()
        };
        
        let key_b64 = general_purpose::STANDARD.encode(&encryption_key);
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&encryption_key));
        
        println!(" Encryption enabled");
        if key.is_none() {
            println!(" Generated key: {}", key_b64);
        }
        
        Some((cipher, key_b64))
    } else {
        None
    };

    // Handle relay mode
    if let Some(relay_server) = relay {
        return send_file_via_relay(file_path, relay_server, num_streams, cipher.map(|(c, k)| (c, k))).await;
    }

    // Get local and external IP
    let local_ip = local_ip().context("Failed to get local IP address")?;
    
    println!(" FastShare Sender");
    println!(" File: {} ({})", file_name, format_file_size(file_size));
    
    if public {
        println!(" Public network mode enabled");
        match get_external_ip().await {
            Ok(external_ip) => {
                println!(" Local IP: {}:{}", local_ip, port);
                println!(" External IP: {}:{}", external_ip, port);
                println!();
                println!("  For public network transfers:");
                println!("   1. Configure port forwarding on your router:");
                println!("      - Forward external port {} to {}:{}", port, local_ip, port);
                println!("   2. Make sure your firewall allows incoming connections on port {}", port);
                println!("   3. Share the external IP with the receiver");
                println!();
                println!("📱 On the receiving device, run:");
                
            }
            Err(_) => {
                println!(" Could not detect external IP. Using local IP: {}:{}", local_ip, port);
                println!(" On the receiving device (same network), run:");
                if let Some((_, ref key_b64)) = cipher {
                    println!(" fastshare receive {} --streams {} --key {}", local_ip, num_streams, key_b64);
                } else {
                    println!(" fastshare receive {} --streams {}", local_ip, num_streams);
                }
            }
        }
    } else {
        println!(" Listening on: {}:{}", local_ip, port);
        println!(" On the receiving device (same network), run:");
        if let Some((_, ref key_b64)) = cipher {
            println!(" fastshare receive {} --streams {} --key {}", local_ip, num_streams, key_b64);
        } else {
            println!(" fastshare receive {} --streams {}", local_ip, num_streams);
        }
    }
    
    println!("Using {} parallel streams", num_streams);
    println!();

    // Start TCP listener
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .context("Failed to bind to port")?;

    println!("Waiting for connection...");

    // Accept main connection
    let (mut main_stream, addr) = listener.accept().await.context("Failed to accept connection")?;
    println!("Connected to: {}", addr);

    // Send file info
    let file_info = FileInfo {
        name: file_name.clone(),
        size: file_size,
        streams: num_streams,
        encrypted: cipher.is_some(),
    };

    let encoded = bincode::serialize(&file_info).context("Failed to serialize file info")?;
    main_stream.write_u32(encoded.len() as u32).await?;
    main_stream.write_all(&encoded).await?;

    // Accept additional stream connections
    let mut streams = vec![main_stream];
    for i in 1..num_streams {
        println!("⏳ Waiting for stream {} connection...", i + 1);
        let (stream, _) = listener.accept().await.context("Failed to accept stream connection")?;
        streams.push(stream);
    }

    println!(" Starting parallel file transfer...");

    // Create progress bar
    let pb = ProgressBar::new(file_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    let progress = Arc::new(Mutex::new(0u64));
    let file_path_arc = Arc::new(file_path);
    let cipher_arc = Arc::new(cipher.map(|(c, _)| c));

    // Calculate chunk ranges for each stream
    let chunk_size_per_stream = file_size / num_streams as u64;
    let mut tasks = Vec::new();

    for (stream_id, mut stream) in streams.into_iter().enumerate() {
        let start_offset = stream_id as u64 * chunk_size_per_stream;
        let end_offset = if stream_id == num_streams - 1 {
            file_size // Last stream handles remainder
        } else {
            (stream_id + 1) as u64 * chunk_size_per_stream
        };

        let file_path_clone = Arc::clone(&file_path_arc);
        let progress_clone = Arc::clone(&progress);
        let pb_clone = pb.clone();
        let cipher_clone = Arc::clone(&cipher_arc);

        let task = tokio::spawn(async move {
            send_file_chunk(stream_id, &mut stream, file_path_clone, start_offset, end_offset, progress_clone, pb_clone, cipher_clone).await
        });

        tasks.push(task);
    }

    // Wait for all streams to complete
    for task in tasks {
        task.await.context("Stream task failed")??;
    }

    pb.finish_with_message("✅ Transfer complete!");
    println!("File sent successfully using {} parallel streams!", num_streams);

    Ok(())
}

async fn send_file_chunk(
    stream_id: usize,
    stream: &mut TcpStream,
    file_path: Arc<PathBuf>,
    start_offset: u64,
    end_offset: u64,
    progress: Arc<Mutex<u64>>,
    pb: ProgressBar,
    cipher: Arc<Option<Aes256Gcm>>,
) -> Result<()> {
    let mut file = File::open(&*file_path).await.context("Failed to open file")?;
    file.seek(std::io::SeekFrom::Start(start_offset)).await.context("Failed to seek file")?;

    let mut buffer = vec![0u8; CHUNK_SIZE];
    let mut current_offset = start_offset;

    while current_offset < end_offset {
        let bytes_to_read = CHUNK_SIZE.min((end_offset - current_offset) as usize);
        buffer.resize(bytes_to_read, 0);

        let bytes_read = file.read(&mut buffer).await.context("Failed to read file")?;
        if bytes_read == 0 {
            break;
        }

        // Encrypt data if cipher is available
        let data_to_send = if let Some(ref cipher) = *cipher {
            encrypt_data(&buffer[..bytes_read], cipher)?
        } else {
            buffer[..bytes_read].to_vec()
        };

        // Send chunk info
        let chunk_info = ChunkInfo {
            stream_id,
            offset: current_offset,
            size: data_to_send.len(),
        };

        let encoded = bincode::serialize(&chunk_info).context("Failed to serialize chunk info")?;
        stream.write_u32(encoded.len() as u32).await?;
        stream.write_all(&encoded).await?;

        // Send chunk data
        stream.write_all(&data_to_send).await.context("Failed to send chunk data")?;

        current_offset += bytes_read as u64;

        // Update progress
        {
            let mut total_progress = progress.lock().await;
            *total_progress += bytes_read as u64;
            pb.set_position(*total_progress);
        }
    }

    Ok(())
}

async fn receive_file(ip: String, port: u16, output_dir: PathBuf, num_streams: usize, key: Option<String>, relay: Option<String>) -> Result<()> {
    println!("FastShare Receiver");
    
    // Handle relay mode
    if let Some(relay_server) = relay {
        return receive_file_via_relay(relay_server, output_dir, key).await;
    }
    
    println!("Connecting to {}:{}...", ip, port);

    // Connect to sender (main stream)
    let mut main_stream = TcpStream::connect(format!("{}:{}", ip, port))
        .await
        .context("Failed to connect to sender")?;

    println!("Connected!");

    // Receive file info - try new format first, fallback to legacy
    let len = main_stream.read_u32().await?;
    let mut buf = vec![0u8; len as usize];
    main_stream.read_exact(&mut buf).await?;
    
    let file_info = if let Ok(info) = bincode::deserialize::<FileInfo>(&buf) {
        // New protocol with parallel streams
        info
    } else if let Ok(legacy_info) = bincode::deserialize::<LegacyFileInfo>(&buf) {
        // Legacy protocol - use single stream
        println!("Detected legacy protocol, using single stream mode");
        FileInfo {
            name: legacy_info.name,
            size: legacy_info.size,
            streams: 1,
            encrypted: false,
        }
    } else {
        return Err(anyhow::anyhow!("Failed to deserialize file info"));
    };

    println!("Receiving: {} ({})", file_info.name, format_file_size(file_info.size));
    if file_info.streams > 1 {
        println!("Using {} parallel streams", file_info.streams);
    }
    if file_info.encrypted {
        println!("File is encrypted");
    }

    // Setup decryption if needed
    let cipher = if file_info.encrypted {
        if let Some(key_str) = &key {
            let encryption_key = key_from_string(key_str)?;
            Some(Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&encryption_key)))
        } else {
            return Err(anyhow::anyhow!("File is encrypted but no decryption key provided. Use --key option."));
        }
    } else {
        None
    };

    // Handle legacy single-stream mode
    if file_info.streams == 1 {
        return receive_file_legacy(main_stream, file_info, output_dir, cipher).await;
    }

    // Create additional stream connections for parallel mode
    let mut streams = vec![main_stream];
    for i in 1..file_info.streams {
        println!("Connecting stream {}...", i + 1);
        let stream = TcpStream::connect(format!("{}:{}", ip, port))
            .await
            .context("Failed to connect additional stream")?;
        streams.push(stream);
    }

    // Create output file
    let output_path = output_dir.join(&file_info.name);
    let output_file = Arc::new(Mutex::new(
        File::create(&output_path)
            .await
            .context("Failed to create output file")?
    ));

    // Create progress bar
    let pb = ProgressBar::new(file_info.size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    println!("Starting parallel file transfer...");

    let progress = Arc::new(Mutex::new(0u64));
    let chunks_buffer = Arc::new(Mutex::new(HashMap::<u64, Vec<u8>>::new()));
    let cipher_arc = Arc::new(cipher);

    // Start receiving tasks for each stream
    let mut tasks = Vec::new();

    for (stream_id, mut stream) in streams.into_iter().enumerate() {
        let progress_clone = Arc::clone(&progress);
        let chunks_buffer_clone = Arc::clone(&chunks_buffer);
        let pb_clone = pb.clone();
        let cipher_clone = Arc::clone(&cipher_arc);

        let task = tokio::spawn(async move {
            receive_file_chunks(stream_id, &mut stream, progress_clone, chunks_buffer_clone, pb_clone, cipher_clone).await
        });

        tasks.push(task);
    }

    // Start a task to write chunks to file in order
    let output_file_clone = Arc::clone(&output_file);
    let chunks_buffer_clone = Arc::clone(&chunks_buffer);
    let write_task = tokio::spawn(async move {
        write_chunks_to_file(output_file_clone, chunks_buffer_clone, file_info.size).await
    });

    // Wait for all receiving tasks to complete
    for task in tasks {
        task.await.context("Receive task failed")??;
    }

    // Wait for write task to complete
    write_task.await.context("Write task failed")??;

    pb.finish_with_message("✅ Transfer complete!");
    
    println!("File received successfully using {} parallel streams!", file_info.streams);
    println!(" Saved to: {}", output_path.display());

    Ok(())
}

async fn receive_file_chunks(
    _stream_id: usize,
    stream: &mut TcpStream,
    progress: Arc<Mutex<u64>>,
    chunks_buffer: Arc<Mutex<HashMap<u64, Vec<u8>>>>,
    pb: ProgressBar,
    cipher: Arc<Option<Aes256Gcm>>,
) -> Result<()> {
    loop {
        // Try to read chunk info length
        let len_result = stream.read_u32().await;
        if len_result.is_err() {
            // Stream closed, we're done
            break;
        }
        let len = len_result?;

        // Read chunk info
        let mut buf = vec![0u8; len as usize];
        stream.read_exact(&mut buf).await?;
        
        let chunk_info: ChunkInfo = bincode::deserialize(&buf)
            .context("Failed to deserialize chunk info")?;

        // Read chunk data
        let mut chunk_data = vec![0u8; chunk_info.size];
        stream.read_exact(&mut chunk_data).await.context("Failed to receive chunk data")?;

        // Decrypt data if cipher is available
        let decrypted_data = if let Some(ref cipher) = *cipher {
            decrypt_data(&chunk_data, cipher)?
        } else {
            chunk_data
        };

        let data_len = decrypted_data.len();

        // Store chunk in buffer
        {
            let mut buffer = chunks_buffer.lock().await;
            buffer.insert(chunk_info.offset, decrypted_data);
        }

        // Update progress (use decrypted data size for accurate progress)
        {
            let mut total_progress = progress.lock().await;
            *total_progress += data_len as u64;
            pb.set_position(*total_progress);
        }
    }

    Ok(())
}

async fn write_chunks_to_file(
    output_file: Arc<Mutex<File>>,
    chunks_buffer: Arc<Mutex<HashMap<u64, Vec<u8>>>>,
    total_size: u64,
) -> Result<()> {
    let mut current_offset = 0u64;

    while current_offset < total_size {
        // Check if we have the next chunk
        let chunk_data = {
            let mut buffer = chunks_buffer.lock().await;
            buffer.remove(&current_offset)
        };

        if let Some(data) = chunk_data {
            // Write chunk to file
            {
                let mut file = output_file.lock().await;
                file.write_all(&data).await.context("Failed to write chunk to file")?;
            }
            current_offset += data.len() as u64;
        } else {
            // Wait a bit for the chunk to arrive
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }
    }

    // Flush the file
    {
        let mut file = output_file.lock().await;
        file.flush().await?;
    }

    Ok(())
}

async fn receive_file_legacy(
    mut stream: TcpStream,
    file_info: FileInfo,
    output_dir: PathBuf,
    cipher: Option<Aes256Gcm>,
) -> Result<()> {
    println!(" Starting legacy file transfer...");

    // Create output file
    let output_path = output_dir.join(&file_info.name);
    let mut output_file = File::create(&output_path)
        .await
        .context("Failed to create output file")?;

    // Create progress bar
    let pb = ProgressBar::new(file_info.size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    // Receive file data in legacy mode (direct stream)
    let mut buffer = vec![0u8; CHUNK_SIZE];
    let mut total_received = 0u64;

    while total_received < file_info.size {
        let bytes_to_read = CHUNK_SIZE.min((file_info.size - total_received) as usize);
        buffer.resize(bytes_to_read, 0);
        
        let bytes_read = stream.read(&mut buffer).await.context("Failed to receive data")?;
        if bytes_read == 0 {
            break;
        }

        let data_to_write = if let Some(ref cipher) = cipher {
            decrypt_data(&buffer[..bytes_read], cipher)?
        } else {
            buffer[..bytes_read].to_vec()
        };

        output_file.write_all(&data_to_write).await.context("Failed to write to file")?;
        
        total_received += data_to_write.len() as u64;
        pb.set_position(total_received);
    }

    output_file.flush().await?;
    pb.finish_with_message("✅ Transfer complete!");
    
    println!(" File received successfully!");
    println!(" Saved to: {}", output_path.display());

    Ok(())
}

async fn run_relay_server(port: u16) -> Result<()> {
    println!(" FastShare Relay Server");
    println!("🌐Listening on port: {}", port);
    
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .context("Failed to bind to port")?;
    
    let sessions: Arc<Mutex<HashMap<String, Arc<Mutex<RelaySession>>>>> = Arc::new(Mutex::new(HashMap::new()));
    
    println!("✅Relay server started! Clients can connect using:");
    println!("   fastshare send <file> --relay <your-server-ip>:{}", port);
    println!("   fastshare receive <session-id> --relay <your-server-ip>:{}", port);
    println!();
    
    loop {
        let (stream, addr) = listener.accept().await?;
        println!("🔗 New connection from: {}", addr);
        
        let sessions_clone = Arc::clone(&sessions);
        tokio::spawn(async move {
            if let Err(e) = handle_relay_connection(stream, sessions_clone).await {
                eprintln!("❌ Relay connection error: {}", e);
            }
        });
    }
}

async fn handle_relay_connection(
    mut stream: TcpStream,
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<RelaySession>>>>>,
) -> Result<()> {
    // Read initial message to determine if this is sender or receiver
    let len = stream.read_u32().await?;
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;
    
    let message: RelayMessage = bincode::deserialize(&buf)?;
    
    match message.message_type.as_str() {
        "sender_init" => {
            let session_id = Uuid::new_v4().to_string();
            let session = Arc::new(Mutex::new(RelaySession {
                session_id: session_id.clone(),
                sender_connected: true,
                receiver_connected: false,
            }));
            
            {
                let mut sessions_guard = sessions.lock().await;
                sessions_guard.insert(session_id.clone(), Arc::clone(&session));
            }
            
            // Send session ID back to sender
            let response = RelayMessage {
                message_type: "session_created".to_string(),
                session_id: session_id.clone(),
                data: vec![],
            };
            
            let encoded = bincode::serialize(&response)?;
            stream.write_u32(encoded.len() as u32).await?;
            stream.write_all(&encoded).await?;
            
            println!("📤 Sender connected, session: {}", session_id);
            
            // Handle sender data forwarding
            handle_sender_relay(stream, session_id, sessions).await?;
        }
        "receiver_init" => {
            let session_id = message.session_id;
            
            let session_exists = {
                let sessions_guard = sessions.lock().await;
                sessions_guard.contains_key(&session_id)
            };
            
            if session_exists {
                println!("Receiver connected to session: {}", session_id);
                handle_receiver_relay(stream, session_id, sessions).await?;
            } else {
                return Err(anyhow::anyhow!("Session not found: {}", session_id));
            }
        }
        _ => {
            return Err(anyhow::anyhow!("Unknown message type: {}", message.message_type));
        }
    }
    
    Ok(())
}

async fn handle_sender_relay(
    mut stream: TcpStream,
    session_id: String,
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<RelaySession>>>>>,
) -> Result<()> {
    // Wait for receiver to connect or timeout
    let timeout = Duration::from_secs(300); // 5 minutes
    let start = std::time::Instant::now();
    
    loop {
        if start.elapsed() > timeout {
            return Err(anyhow::anyhow!("Timeout waiting for receiver"));
        }
        
        let receiver_connected = {
            let sessions_guard = sessions.lock().await;
            if let Some(session) = sessions_guard.get(&session_id) {
                let session_guard = session.lock().await;
                session_guard.receiver_connected
            } else {
                false
            }
        };
        
        if receiver_connected {
            break;
        }
        
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    
    println!("Starting relay for session: {}", session_id);
    
    // Forward data from sender to receiver
    let mut buffer = vec![0u8; CHUNK_SIZE];
    loop {
        match stream.read(&mut buffer).await {
            Ok(0) => break, // Connection closed
            Ok(n) => {
                // Forward to receiver (implementation would need a channel or similar)
                // This is a simplified version - in practice you'd need proper session management
                println!("Forwarding {} bytes", n);
            }
            Err(e) => return Err(e.into()),
        }
    }
    
    Ok(())
}

async fn handle_receiver_relay(
    mut stream: TcpStream,
    session_id: String,
    sessions: Arc<Mutex<HashMap<String, Arc<Mutex<RelaySession>>>>>,
) -> Result<()> {
    // Mark receiver as connected
    {
        let sessions_guard = sessions.lock().await;
        if let Some(session) = sessions_guard.get(&session_id) {
            let mut session_guard = session.lock().await;
            session_guard.receiver_connected = true;
        }
    }
    
    println!("Receiver ready for session: {}", session_id);
    
    // Forward data from sender to receiver
    // This is a simplified implementation
    Ok(())
}

async fn send_file_via_relay(
    file_path: PathBuf,
    relay_server: String,
    num_streams: usize,
    cipher: Option<(Aes256Gcm, String)>,
) -> Result<()> {
    println!(" Connecting to relay server: {}", relay_server);
    
    let mut stream = TcpStream::connect(&relay_server).await?;
    
    // Send sender init message
    let init_message = RelayMessage {
        message_type: "sender_init".to_string(),
        session_id: String::new(),
        data: vec![],
    };
    
    let encoded = bincode::serialize(&init_message)?;
    stream.write_u32(encoded.len() as u32).await?;
    stream.write_all(&encoded).await?;
    
    // Receive session ID
    let len = stream.read_u32().await?;
    let mut buf = vec![0u8; len as usize];
    stream.read_exact(&mut buf).await?;
    
    let response: RelayMessage = bincode::deserialize(&buf)?;
    let session_id = response.session_id;
    
    println!("✅ Session created: {}", session_id);
    println!("📱 Share this session ID with the receiver:");
    if let Some((_, ref key_b64)) = cipher {
        println!(" fastshare receive {} --relay {} --key {}", session_id, relay_server, key_b64);
    } else {
        println!(" fastshare receive {} --relay {}", session_id, relay_server);
    }
    println!();
    println!("⏳ Waiting for receiver to connect...");
    
    // Continue with file transfer through relay
    // This would need full implementation of the relay protocol
    
    Ok(())
}

async fn receive_file_via_relay(
    relay_server: String,
    output_dir: PathBuf,
    key: Option<String>,
) -> Result<()> {
    println!("🔄 Connecting to relay server: {}", relay_server);
    
    // Extract session ID from the "IP" parameter (when using relay mode)
    let session_id = relay_server.split(':').next().unwrap_or(&relay_server).to_string();
    
    let mut stream = TcpStream::connect(&relay_server).await?;
    
    // Send receiver init message
    let init_message = RelayMessage {
        message_type: "receiver_init".to_string(),
        session_id,
        data: vec![],
    };
    
    let encoded = bincode::serialize(&init_message)?;
    stream.write_u32(encoded.len() as u32).await?;
    stream.write_all(&encoded).await?;
    
    println!("✅ Connected to relay session");
    
    // Continue with file reception through relay
    // This would need full implementation of the relay protocol
    
    Ok(())
}