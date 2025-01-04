# Goodbye-DPI
This is a crossplatform Deep Packet Inspection circumvention utility inspired by [Goodbye DPI](https://github.com/ValdikSS/GoodbyeDPI)
The idea is to fragment first data packet on TCP level using blacklist and Aho-Corasick algorithm.

## Table of Contents

- [Installation](#installation)
- [Usage](#usage)
- [Blacklists](#blacklists)
- [Contributing](#contributing)

## Installation
To install this project, follow these steps:

#### Prerequisites
Make sure you have the following installed on your system:

- [Rust](https://www.rust-lang.org/tools/install)
- [Cargo](https://doc.rust-lang.org/cargo/getting-started/installation.html) (comes with Rust)

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

#### Conclusion
The Blacklists feature helps you manage your proxy settings dynamically, ensuring that you can access the content you need without being hindered by IP blocks. 

## Contributing
If you would like to contribute to this project, please follow these steps:

1. Fork the repository.
2. Create a new branch (git checkout -b feature-branch).
3. Make your changes and commit them (git commit -m 'Add new feature').
4. Push to the branch (git push origin feature-branch).
5. Create a new Pull Request.

