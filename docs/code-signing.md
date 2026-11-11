# Code signing policy

Dish has applied for free code signing provided by
[SignPath.io](https://about.signpath.io), certificate by
[SignPath Foundation](https://signpath.org). The application is pending, so
current Windows builds are **unsigned**. The intent is to publish Windows builds
signed with Authenticode, so Windows and endpoint security products can tell
where the binary came from instead of treating it as an unknown file. Once
signing is enabled, the certificate belongs to the SignPath Foundation, which is
therefore the publisher of record for signed Dish releases.

## Current state and what the user sees

The published Windows builds are pre-releases and are **not code-signed yet**.
SmartScreen **will** warn when the installer runs. The user can check the
published SHA-256 first; if it matches and the user accepts the risk, the option
to continue is **More info → Run anyway**. That is a deliberate risk decision,
not a safe default, and Dish does not ask anyone to disable SmartScreen,
Defender or another protection to install it.

The SignPath Foundation application was submitted on 2026-10-28 and had no
response at the time of writing. No date is promised for signing. Until it is
approved and configured, the table and signature commands below describe the
intended signed release, not the current download.

## What is signed

| Artifact | Signed today | Notes |
|---|---|---|
| `dish-<version>-windows-x86_64-setup.exe` | no | planned; pending SignPath Foundation onboarding |
| `dish-<version>-windows-x86_64.msi` | no | planned; pending SignPath Foundation onboarding |
| `dish.exe` (inside both installers) | no | planned; to be signed before packaging |
| `.tar.gz` / `.deb` / `.rpm` / `.AppImage` (Linux) | no | Linux has no equivalent of Authenticode; integrity comes from the published SHA-256 |

When signing is enabled, it will happen in the release workflow, on
GitHub-hosted runners, on artifacts built from this repository by that same
workflow. Only `release` events will be signed; pull-request builds stay
unsigned. Every signing request will require a human approval in SignPath before
the certificate is used.

## Verifying a download

While builds are unsigned, the published SHA-256 is the only check available.
Compare the file with its `.sha256` sidecar:

```cmd
certutil -hashfile dish-<version>-windows-x86_64-setup.exe SHA256
```

```bash
sha256sum -c dish-<version>-linux-x86_64.tar.gz.sha256
```

`Get-AuthenticodeSignature` currently reports `NotSigned`, and `signtool verify`
fails; that is expected. Once signing is enabled, compare the hash and then check
the signature:

```powershell
Get-AuthenticodeSignature .\dish-<version>-windows-x86_64-setup.exe | Format-List
```

```cmd
signtool verify /pa /v dish-<version>-windows-x86_64-setup.exe
```

A signed release reports `Status: Valid` and a `SignerCertificate` issued to
SignPath Foundation.

## What still has to happen

- SignPath Foundation must approve the application and provision the
  organization, project, signing policy and artifact configuration. The
  repository variables and `SIGNPATH_API_TOKEN` secret must point at them.
- A trusted Authenticode certificate must be available to the release workflow.
  No certificate is available to it today, so the current release is unsigned.
- `scripts/sign-windows.ps1` must fail loudly when signing is configured but the
  credential is missing. Today it exits 0 when both `WINDOWS_CERT_*` secrets are
  absent, and only throws when the configuration is partial.
- `dish.exe` must be signed before packaging, and the installer and MSI after
  packaging. The `.sha256` sidecars must be recomputed after signing, never
  before.
- `signtool verify /pa /v` must report a valid signature for each signed
  artifact, and the release job must fail otherwise.
- This table, the README download notice and the landing page must switch from
  "unsigned" to the actual signed state in the release that first carries a
  signature.

## When signing is enabled

The release that first carries a signature will update this policy, the README
and the download page together, including the attribution required by SignPath:
"Free code signing provided by SignPath.io, certificate by SignPath Foundation."
Signing does not make warnings disappear immediately — hash reputation takes
time — so no release note will promise a warning-free download.

## Team roles

- Committers and reviewers: [@cleisonsantos](https://github.com/cleisonsantos)
- Approvers: [@cleisonsantos](https://github.com/cleisonsantos)

Everyone with write access on this repository is required to use multi-factor
authentication, on GitHub and on SignPath.
