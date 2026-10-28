# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

Dish publishes Windows builds signed with Authenticode, so Windows and endpoint
security products can tell where the binary came from instead of treating it as
an unknown file. The certificate belongs to the SignPath Foundation, which is
therefore the publisher of record for signed Dish releases.

## What is signed

| Artifact | Signed | Notes |
|---|---|---|
| `dish-<version>-windows-x86_64-setup.exe` | yes | NSIS installer |
| `dish-<version>-windows-x86_64.msi` | yes | WiX/Windows Installer package |
| `dish.exe` (inside both installers) | yes | signed before packaging, so the installed binary carries the signature |
| `.tar.gz` / `.deb` / `.rpm` / `.AppImage` (Linux) | no | Linux has no equivalent of Authenticode; integrity comes from the published SHA-256 |

Signing happens in the release workflow, on GitHub-hosted runners, on artifacts
built from this repository by that same workflow. Only `release` events are
signed; pull-request builds are unsigned.

Every signing request requires a human approval in SignPath before the
certificate is used.

## Verifying a download

Compare the file against the published `.sha256`, then check the signature:

```powershell
Get-AuthenticodeSignature .\dish-<version>-windows-x86_64-setup.exe | Format-List
```

```cmd
signtool verify /pa /v dish-<version>-windows-x86_64-setup.exe
```

A signed release reports `Status: Valid` and `SignerCertificate` issued to
SignPath Foundation.

## Team roles

- Committers and reviewers: [@cleisonsantos](https://github.com/cleisonsantos)
- Approvers: [@cleisonsantos](https://github.com/cleisonsantos)

Everyone with write access on this repository is required to use multi-factor
authentication, on GitHub and on SignPath.
