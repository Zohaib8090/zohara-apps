# zohara-apps

Rust workspace for the user-facing tools that ship with Zohara OS but
aren't part of the Settings app. Each crate is a separate Arch package
on the [Zohara Packages OTA repo](https://github.com/Zohaib8090/zohara-packages).

## Crates

| Crate        | Binary                | Status     | Replaces |
|--------------|-----------------------|------------|----------|
| `welcome`    | `zohara-welcome`      | ✅ ported   | `zohara-welcome` (PyQt5) |
| `update`     | `zohara-update`       | ⏳ TODO     | `zohara-update` (PyQt5) |
| `usermgr`    | `zohara-usermgr`      | ⏳ TODO     | `zohara-usermgr` (PyQt5) |
| `migrate`    | `zohara-migrate`      | ⏳ TODO     | `zohara-migrate` (PyQt5) |
| `common`     | (library)             | 🟡 stub    | shared helpers |

The end goal: **zero Python in the live ISO**. The Settings app is
already Rust (`Zohaib8090/zohara-settings`); this workspace replaces
the four remaining Python/PyQt5 user-facing tools.

## Build

```sh
cargo build --release
# binaries in target/release/zohara-{welcome,update,usermgr,migrate}
```

## CI

`build.yml` builds all 4 binaries and uploads them to the matching
channel release in `Zohaib8090/zohara-packages`:
- push to `main` → stable
- push to `beta` → beta
- workflow_dispatch with channel=alpha → alpha

The `PACKAGES_DISPATCH_TOKEN` secret (a PAT with `repo` scope on the
packages repo) lets the workflow cross-dispatch and update
`zohara.db` + `apps.json` automatically. Without it, the binaries
upload but the channel's package database isn't refreshed.
