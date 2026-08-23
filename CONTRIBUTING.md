<!-- CONTRIBUTING -->
## Contributing

Any contributions you make are greatly appreciated.

Needed [Git](https://git-scm.com/downloads) for Contributing.

1. [Fork the Project](https://github.com/greluc/AnotherCrewLink/fork).
2. Create your Feature Branch. (`git checkout -b feature/AmazingFeature`)
3. Commit your Changes. (`git commit -m 'Add some AmazingFeature'`)
4. Push to the Branch. (`git push origin feature/AmazingFeature`)
5. Open a Pull Request.

### Development

You only need to follow the below instructions if you are trying to modify this software. Otherwise, please download the latest version from the [GitHub releases](https://github.com/greluc/AnotherCrewLink/releases).

Server code is located at [greluc/AnotherCrewLink-server](https://github.com/greluc/AnotherCrewLink-server). Please use a local server for development purposes.

### Prerequisites

Three native modules are compiled from the sources vendored under `native/`, so a
C++ toolchain is needed alongside Node.js:

* [Node.js](https://nodejs.org/en/download/) 22 or later
* [Python](https://www.python.org/downloads/) 3, for node-gyp
* **Windows**: Visual Studio with the "Desktop development with C++" workload
* **Linux**: `build-essential`, `libxcb1-dev`, `libx11-dev`

### Setup

1. Clone the repo
```sh
git clone https://github.com/greluc/AnotherCrewLink.git
cd AnotherCrewLink
```
2. Install NPM packages
```sh
npm ci
```
3. Run the project
```sh
npm run dev
```

Before opening a pull request, run the same checks CI does:
```sh
npm run lint && npm run typecheck && npm test
```

<!-- TRANSLATING -->
## Translating

AnotherCrewLink supports other languages, that is, you can use AnotherCrewLink without any problem of not understanding a part in English, but with that we need help with translations because nobody is born knowing everything languages.

Any translations you make are greatly appreciated.

1. [Fork the Project](https://github.com/greluc/AnotherCrewLink/fork).
2. Create your Translation Branch.
3. Go to static **->** locales **->** en **->** translation.json and Download this file.
4. Open the translation.json with your text editor of preference.
5. Edit the file but not edit this parts like: "gamehostonly", "inlobbyonly", just translate the text.
6. Create a folder with the acronym of your language that you translated with translation.json inside the folder.
7. Throw everything to your fork.
8. Open a Pull Request.

[contributors-shield]: https://img.shields.io/github/contributors/greluc/AnotherCrewLink?label=Contributors&logo=GitHub
[contributors-url]: https://github.com/greluc/AnotherCrewLink/graphs/contributors
