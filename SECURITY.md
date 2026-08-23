# Security Policy

## Supported Versions

| Version | Supported? |
|---|:---:|
| [AnotherCrewLink](https://github.com/greluc/AnotherCrewLink) | yes |
| [AnotherCrewLink Server](https://github.com/greluc/AnotherCrewLink-server) | yes |
| [BetterCrewLink](https://github.com/OhMyGuus/BetterCrewLink) (upstream) | reported there |
| [CrewLink](https://github.com/ottomated/CrewLink) (original) | no |

## Reporting a Vulnerability

Report vulnerabilities privately through GitHub's security advisories on
[greluc/AnotherCrewLink](https://github.com/greluc/AnotherCrewLink/security/advisories/new),
or by email to lucas.greuloch@pm.me. Please do not open a public issue for
a vulnerability.

Because this application reads another process's memory and installs a global
keyboard hook, reports touching the native modules under `native/` are especially
welcome.
