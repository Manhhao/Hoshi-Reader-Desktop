<div align="center">

# Hoshi Reader Desktop

![Language](https://img.shields.io/github/languages/top/Manhhao/Hoshi-Reader-Desktop)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey)
![License](https://img.shields.io/github/license/Manhhao/Hoshi-Reader-Desktop)

Desktop version of [Hoshi Reader](https://github.com/Manhhao/Hoshi-Reader) and [Hoshi Reader Android](https://github.com/HuangAntimony/Hoshi-Reader-Android) made using Tauri and Svelte.

</div>

<table>
  <tr>
    <td width="33%"><img src=".github/screenshots/home.png" alt="Home"></td>
    <td width="33%"><img src=".github/screenshots/popup-sasayaki.png" alt="Dictionary popup"></td>
    <td width="33%"><img src=".github/screenshots/shelf.png" alt="Shelf"></td>
  </tr>
  <tr>
    <td width="33%"><img src=".github/screenshots/statistics.png" alt="Statistics"></td>
    <td width="33%"><img src=".github/screenshots/gallery.png" alt="Gallery"></td>
    <td width="33%"><img src=".github/screenshots/dictionary.png" alt="Dictionaries"></td>
  </tr>
</table>

## Download

Get the latest version from [GitHub Releases](https://github.com/Manhhao/Hoshi-Reader-Desktop/releases/latest).

Requires macOS 15 or newer on Apple Silicon, or Windows 10/11.

## Development

### Prerequisites

- [Rust](https://rustup.rs/) 1.88+
- [Node.js](https://nodejs.org/) 22.12+
- [pnpm](https://pnpm.io/)
- [CMake](https://cmake.org/) 3.24+
- A C++23 compiler
  - macOS: Xcode Command Line Tools
  - Windows: Visual Studio with *Desktop development with C++ workload* and *C++ Clang tools for Windows*

### Run

```sh
pnpm install
pnpm tauri dev
```

## Issues

Please open an issue [here](https://github.com/Manhhao/Hoshi-Reader-Desktop/issues).

## License

Distributed under the GNU General Public License v3.0. See [LICENSE](LICENSE) for more information.
