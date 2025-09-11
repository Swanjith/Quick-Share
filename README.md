# FastShare 🚀

A lightning-fast command-line file sharing tool for transferring files between devices on the same network.

## Features

- ⚡ **Ultra-fast transfers** with optimized TCP streaming
- 🔀 **Parallel streaming** - multiple concurrent connections for maximum speed
- 📊 **Real-time progress bar** with transfer speed and ETA
- 🌐 **Cross-platform** - works on Windows, macOS, and Linux
- 🔧 **Simple CLI** - just two commands to send and receive
- 🛡️ **Local network only** - secure transfers within your WiFi network

## Installation

1. Make sure you have [Rust](https://rustup.rs/) installed
2. Clone this repository:
   ```bash
   git clone https://github.com/yourusername/fastshare
   cd fastshare
   ```
3. Build the project:
   ```bash
   cargo build --release
   ```
4. The binary will be available at `target/release/fastshare`

## Usage

### Sending a file

On the device that has the file you want to share:

```bash
fastshare send path/to/your/file.txt
```

This will:

- Display your local IP address
- Start listening for connections
- Show a command for the receiving device

Example output:

```
🚀 FastShare Sender
📁 File: document.pdf (2,456,789 bytes)
🌐 Listening on: 192.168.1.100:8080
🔀 Using 4 parallel streams
📱 On the receiving device, run:
   fastshare receive 192.168.1.100 --streams 4

⏳ Waiting for connection...
```

### Receiving a file

On the device where you want to receive the file:

```bash
fastshare receive 192.168.1.100
```

This will connect to the sender and download the file to the current directory.

Example output:

```
🚀 FastShare Receiver
🔗 Connecting to 192.168.1.100:8080...
✅ Connected!
📁 Receiving: document.pdf (2,456,789 bytes)
� Usirng 4 parallel streams
📥 Starting parallel file transfer...
⠋ [00:00:01] [##########>           ] 1.2MB/2.4MB (1.2MB/s, 00:00:01)
```

## Public Network Transfers

FastShare supports transfers over the internet using the `--public` flag, which automatically detects your external IP and provides setup instructions.

### Sending over Public Networks

```bash
fastshare send your-file.txt --public
```

This will:

- Automatically detect your external IP address
- Show both local and external IP addresses
- Provide router configuration instructions
- Display the exact command for the receiver

Example output:

```
🚀 FastShare Sender
📁 File: document.pdf (2.4 MB)
🌍 Public network mode enabled
🌐 Local IP: 192.168.1.100:8080
🌍 External IP: 203.0.113.45:8080

⚠️  For public network transfers:
   1. Configure port forwarding on your router:
      - Forward external port 8080 to 192.168.1.100:8080
   2. Make sure your firewall allows incoming connections on port 8080
   3. Share the external IP with the receiver

📱 On the receiving device, run:
 fastshare receive 203.0.113.45 --streams 10
```

### Public Network Setup Requirements

1. **Router Port Forwarding**: Configure your router to forward the specified port to your local machine
2. **Firewall Configuration**: Allow incoming connections on the port
3. **Security Considerations**: Use encryption for sensitive files over public networks

### Secure Public Transfers

For added security over public networks, use encryption:

```bash
# Generate a random encryption key
fastshare send file.txt --public --encrypt

# Use a custom encryption key
fastshare send file.txt --public --key "your-base64-encoded-key"
```

The receiver will need the encryption key:

```bash
fastshare receive 203.0.113.45 --key "your-base64-encoded-key"
```

### Relay Server Mode

For networks behind NAT or restrictive firewalls, use relay mode:

```bash
# Run a relay server (on a public server with open ports)
fastshare relay --port 8080

# Send via relay
fastshare send file.txt --relay your-relay-server.com:8080

# Receive via relay (use the session ID provided by sender)
fastshare receive session-id --relay your-relay-server.com:8080
```

## Command Options

### Send Command

```bash
fastshare send <FILE> [OPTIONS]
```

Options:

- `-p, --port <PORT>` - Custom port (default: 8080)
- `-s, --streams <COUNT>` - Number of parallel streams (default: 10)
- `-e, --encrypt` - Enable encryption with random key
- `-k, --key <KEY>` - Custom encryption key (base64 encoded, 32 bytes)
- `--public` - Enable public network mode (shows external IP)
- `--relay <SERVER>` - Use relay server for NAT traversal

### Receive Command

```bash
fastshare receive <IP> [OPTIONS]
```

Options:

- `-p, --port <PORT>` - Custom port (default: 8080)
- `-o, --output <DIR>` - Output directory (default: current directory)
- `-s, --streams <COUNT>` - Number of parallel streams (default: 10)
- `-k, --key <KEY>` - Decryption key (required if sender uses encryption)
- `--relay <SERVER>` - Use relay server for NAT traversal

### Relay Command

```bash
fastshare relay [OPTIONS]
```

Options:

- `-p, --port <PORT>` - Port to listen on (default: 8080)

## Examples

### Send a file on a custom port with more streams

```bash
fastshare send my-file.zip --port 9000 --streams 8
```

### Receive to a specific directory with parallel streams

```bash
fastshare receive 192.168.1.100 --output ~/Downloads --streams 8
```

### Transfer between different platforms

```bash
# On Windows (sender)
fastshare send C:\Users\John\Documents\presentation.pptx

# On macOS (receiver)
fastshare receive 192.168.1.100 --output ~/Desktop
```

### Public network transfers with encryption

```bash
# Sender (generates random key)
fastshare send secret-document.pdf --public --encrypt

# Receiver (using the key provided by sender)
fastshare receive 203.0.113.45 --key "generated-base64-key-here"
```

### Using relay server for NAT traversal

```bash
# Set up relay server (on a VPS or cloud server)
fastshare relay --port 8080

# Send via relay
fastshare send large-file.zip --relay relay.example.com:8080

# Receive via relay (use session ID from sender)
fastshare receive abc123-session-id --relay relay.example.com:8080
```

## Performance

FastShare is optimized for maximum speed:

- **Parallel streaming** with multiple concurrent TCP connections
- Uses efficient 64KB chunks for optimal throughput
- Minimal protocol overhead
- Direct TCP streaming without compression (for maximum speed)
- Progress tracking with minimal performance impact
- Automatic chunk ordering and reassembly

Typical transfer speeds on a modern network:

- **Local WiFi (5GHz) with 4 streams**: 100-200 MB/s
- **Local WiFi (2.4GHz) with 4 streams**: 25-50 MB/s
- **Ethernet with 8 streams**: 200+ MB/s

The parallel streaming can significantly improve speeds, especially on high-bandwidth networks where a single TCP connection might not saturate the available bandwidth.

## Security

FastShare offers flexible security options depending on your use case:

### Local Network Mode (Default)

- Designed for trusted local networks
- No authentication required
- No encryption by default (prioritizes speed)
- Direct peer-to-peer transfers

### Public Network Mode

- Supports internet transfers with `--public` flag
- **Encryption strongly recommended** for public transfers
- AES-256-GCM encryption available with `--encrypt` or `--key` options
- Requires proper router and firewall configuration

### Security Best Practices

**For Local Networks:**

- Use default mode on trusted WiFi networks
- Ensure network is password-protected

**For Public Networks:**

- Always use `--encrypt` flag for sensitive files
- Share encryption keys through secure channels (not over the same network)
- Consider using relay servers to avoid exposing your IP
- Monitor network traffic and close connections after transfer

**General:**

- Files are transferred directly without cloud storage
- No persistent data storage on relay servers
- Temporary session data only

⚠️ **Warnings**:

- Only use unencrypted mode on trusted networks
- For public transfers, always use encryption for sensitive data
- Relay servers can see encrypted data but cannot decrypt without keys

## Troubleshooting

### Connection Issues

**Local Network:**

- Ensure both devices are on the same WiFi network
- Check if firewall is blocking the port (default: 8080)
- Try a different port using `--port` option

**Public Network:**

- Verify port forwarding is configured correctly on router
- Check that external IP is accessible (use online port checkers)
- Ensure firewall allows incoming connections on the specified port
- Try using relay mode if direct connection fails

**Relay Mode:**

- Verify relay server is running and accessible
- Check that relay server port is open to the internet
- Ensure both sender and receiver can reach the relay server

### Permission Issues

- Make sure you have read permissions for the source file
- Ensure write permissions for the destination directory
- On some systems, binding to ports < 1024 requires admin privileges

### Encryption Issues

- Ensure both sender and receiver use the same encryption key
- Verify the key is properly base64 encoded (32 bytes when decoded)
- Check that the `--key` parameter is provided to the receiver when sender uses encryption

### Large Files

- FastShare can handle files of any size
- For very large files (>1GB), ensure stable network connection
- Monitor available disk space on receiving device
- Consider using more streams (`--streams`) for better performance on high-bandwidth connections

### Network Performance

- Increase `--streams` count for faster transfers on high-bandwidth networks
- Decrease streams if experiencing connection issues
- For public transfers, network speed depends on both upload and download bandwidth

## Contributing

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/amazing-feature`)
3. Commit your changes (`git commit -m 'Add some amazing feature'`)
4. Push to the branch (`git push origin feature/amazing-feature`)
5. Open a Pull Request

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- Built with [Rust](https://www.rust-lang.org/) for maximum performance
- Uses [Tokio](https://tokio.rs/) for async networking
- Progress bars powered by [indicatif](https://github.com/console-rs/indicatif)
- CLI interface with [clap](https://github.com/clap-rs/clap)
