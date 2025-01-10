# Goodbye-DPI
This is a crossplatform Deep Packet Inspection circumvention utility inspired by [Goodbye DPI](https://github.com/ValdikSS/GoodbyeDPI)
The idea is to fragment first data packet on TCP level using blacklist and Aho-Corasick algorithm.

## Table of Contents

- [Installation](#installation)
- [Usage](#usage)
- [Blacklists](#blacklists)
- [DNS](#dns)
- [Contributing](#contributing)

## Installation
To install this project, follow these steps:

#### Prerequisites
Make sure you have the following installed on your system:

- [Rust](https://www.rust-lang.org/tools/install)
- [Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html) (comes with Rust)


```
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

#### Clone the Repository
First, clone the repository to your local machine:

```bash
git clone https://github.com/nstrmx/Goodbye-DPI.git
cd Goodbye-DPI
```

## Usage
#### Run the Project
You can run the project with the following command while you are in Goodbye-DPI directory:

```bash
cargo run --release
```
#### Configure your browser's proxy settings
After running the program, you need to configure your browser's proxy settings:

1. Set your browser's HTTP and HTTPS proxy to `127.0.0.1:8080`.
    - chrome example: `google-chrome --proxy-server=127.0.0.1:8080`
2. Open your browser and navigate to YouTube.
3. Check if YouTube is unblocked.
This setup allows the program to route your browser's traffic through the specified proxy.

## Blacklists
#### Overview
The "Blacklists" feature enhances your browsing experience by allowing you to route specific traffic through different proxy servers based on defined blacklists. This is particularly useful for accessing content that may be blocked or restricted by your default proxy settings.

#### How It Works
With the Blacklists feature, you can create multiple blacklists that map specific hosts to different proxy servers. This allows you to customize your browsing experience by ensuring that certain content is accessed through a designated proxy, while all other traffic continues to use the default proxy.

#### Example Use Case
Imagine you are trying to access Twitter, which works perfectly with your default proxy server at 127.0.0.1:8080. However, you find that Twitter's CDN, hosted on twimg.com, is blocked by IP. To resolve this issue, you can create a separate blacklist for twimg.com and route it through the Tor proxy.

#### Configuration Steps
1. Create a blacklist and define it in your configuration YAML file.
1. Specify the proxy server you want to use for the blacklist. In this case, you would map twimg.com to the Tor proxy at `socks5://127.0.0.1:9050`.
Your configuration might look something like this:
```yaml
# Your default proxy (Goodbye-DPI)
buffer_size: 4096
dns:
  provider: cloudflare
  protocol: https
default: 
  url: "tcp//127.0.0.1:8080"
  blacklist: "default_blacklist"
# Additional proxy servers
servers:
  - url: "socks5://127.0.0.1:9050"
    blacklist: "tor_blacklist"
  - url: "tcp://127.0.0.1:8881"
    blacklist: "other_blacklist"
    # and so on
```
3. Run the following command while in Goodbye-DPI directory:
```bash
cargo run --release -- --config your_config.yaml
```
With this configuration, any requests to twimg.com will be routed through the Tor proxy, while all other traffic will continue to be processed by the default proxy at 127.0.0.1:8080. This allows you to bypass restrictions on specific hosts without affecting your overall browsing experience.

Note: config reloads automaticaly.

## DNS
This proxy server supports configurable DNS resolution, allowing you to choose between multiple popular DNS providers. Each provider can be configured to use specific protocols, including TLS, HTTPS, TCP, or UDP.

#### Supported DNS Providers
* Google Public DNS
* Cloudflare DNS
* Quad9 DNS

Each provider offers robust and secure DNS services, and the protocol you choose determines how the DNS queries are sent to the provider.

#### Available Protocols
* TLS: Secure and encrypted DNS resolution over TLS.
* HTTPS: DNS resolution using the DoH (DNS-over-HTTPS) protocol.
* TCP/UDP: Standard DNS resolution using TCP or UDP. These share the same configuration and are considered the "default" behavior.

#### Configuration
You can configure the DNS provider and protocol in your application settings or configuration file. Below is an example of how to configure the DNS feature:

Example Configuration
```yaml
buffer_size: 4096
dns:
  provider: cloudflare
  protocol: https
default: 
  url: "tcp//127.0.0.1:8080"
  blacklist: "default_blacklist"
````
provider: Specifies the DNS service to use (cloudflare, google, or quad9).
protocol: Defines the protocol to be used for DNS queries (tls, https, or default).

#### Default Protocol Behavior
If the provider is set to default, the server will use google server for DNS resolution.
If the protocol is set to default, the server will use either TCP or UDP for DNS resolution, depending on the network and query type.

#### Benefits of Configurable DNS
Flexibility: Easily switch between DNS providers based on your preferences or requirements.
Security: Use TLS or HTTPS for encrypted DNS queries, ensuring privacy and protection against DNS spoofing.
Performance: Choose a protocol that suits your network environment for optimal performance.
By leveraging this feature, you can ensure efficient and secure DNS resolution for your proxy server while maintaining flexibility in your configurations.

## Contributing
If you would like to contribute to this project, please follow these steps:

1. Fork the repository.
2. Create a new branch (git checkout -b feature-branch).
3. Make your changes and commit them (git commit -m 'Add new feature').
4. Push to the branch (git push origin feature-branch).
5. Create a new Pull Request.

## License
GNU General Public License v3.0
