# Goodbye-DPI
This is a crossplatform Deep Packet Inspection circumvention utility inspired by [Goodbye DPI](https://github.com/ValdikSS/GoodbyeDPI)
The idea is to fragment first data packet on TCP level using blacklist and Aho-Corasick algorithm.

## Table of Contents

- [Installation](#installation)
- [Usage](#usage)
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

1. Set your browser's HTTP and HTTPS proxy to `127.0.0.1:8881`.
    - chrome example: `google-chrome --proxy-server=127.0.0.1:8881`
2. Open your browser and navigate to YouTube.
3. Check if YouTube is unblocked.
This setup allows the program to route your browser's traffic through the specified proxy.

## Contributing
If you would like to contribute to this project, please follow these steps:

1. Fork the repository.
2. Create a new branch (git checkout -b feature-branch).
3. Make your changes and commit them (git commit -m 'Add new feature').
4. Push to the branch (git push origin feature-branch).
5. Create a new Pull Request.

