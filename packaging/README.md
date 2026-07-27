# Packaging

## Local install (any distro)

```
./packaging/install.sh
```

Builds release binaries and installs `scd`, `sc`, `sc-gui` to `~/.local/bin`
plus a desktop launcher. Then start the daemon on login and bind a hotkey.

### Hyprland

```
exec-once = ~/.local/bin/scd
bind = SUPER, F10, exec, ~/.local/bin/sc save
```

### systemd user service (alternative to exec-once)

```
mkdir -p ~/.config/systemd/user
cp packaging/systemd/simpleclip.service ~/.config/systemd/user/
systemctl --user enable --now simpleclip.service
```

The daemon needs the Wayland/portal session env; `exec-once` inherits it from the
compositor directly and is the most reliable option.

## Distributable packages (planned)

- AUR (`PKGBUILD`)
- AppImage
- winget (Windows, after the Phase 4 backend)
