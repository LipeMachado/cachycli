# Releasing

Everything is driven by the tag. `[workspace.package] version` in `Cargo.toml`
is the single place to bump; `cachycli version` reads it, and the release
workflow names its assets with it.

```bash
# 1. bump the version
$EDITOR Cargo.toml

# 2. tag and push — the workflow builds and uploads the archives
git commit -am "Release 0.2.0"
git tag v0.2.0
git push origin main --tags
```

`.github/workflows/release.yml` builds five archives
(`cachycli-<version>-<os>-<arch>.tar.gz`, `.zip` on Windows) for Linux, macOS
and Windows on x86_64 and aarch64, with the binary at the archive root — the
names `install.sh` and `install.ps1` expect.

## Rehearsing without publishing

The installers download from `https://github.com/<repo>/releases/download`, so
point them somewhere else and run the whole path locally:

```bash
mkdir -p /tmp/relbase/v0.1.0
cp dist/cachycli-0.1.0-linux-x86_64.tar.gz /tmp/relbase/v0.1.0/
(cd /tmp/relbase && python3 -m http.server 8899 &)

CACHY_RELEASE_BASE=http://127.0.0.1:8899 CACHY_VERSION=0.1.0 \
CACHY_INSTALL_DIR=/tmp/relinstall bash install.sh
/tmp/relinstall/cachycli --version
```

`CACHY_UPDATE_REPO` does the same for `cachycli update` and the installers:
point it at a fork and they follow.
