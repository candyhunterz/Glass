# Linux Installation

## Debian package (recommended)

1. Download the latest `.deb` file from the [GitHub Releases](https://github.com/candyhunterz/Glass/releases) page.
2. Install with dpkg:

```bash
sudo dpkg -i glass_*.deb
```

3. Launch Glass from your application menu or by running `glass` in a terminal.

## Dependencies

The deb package declares its dependencies automatically, but if installing manually, ensure you have:

- GPU drivers with **Vulkan** or **OpenGL 3.3+** support
- Standard terminal libraries (typically already present on desktop Linux)
- A running display server (X11 or Wayland)

### GPU driver troubleshooting

If Glass fails to start with a GPU error:

```bash
# Check Vulkan support
vulkaninfo --summary

# For NVIDIA, ensure proprietary drivers are installed
sudo apt install nvidia-driver-535

# For AMD/Intel, mesa drivers are usually sufficient
sudo apt install mesa-vulkan-drivers
```

## System requirements

- 64-bit Linux distribution (Ubuntu 20.04+, Debian 11+, or equivalent)
- GPU with Vulkan or OpenGL 3.3+ support
- x86_64 processor
