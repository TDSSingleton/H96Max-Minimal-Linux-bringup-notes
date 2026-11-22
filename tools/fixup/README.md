# Fixup kit

Repairs for the known failure modes of the box. The full symptom → cause → fix table is
[docs/log/troubleshooting.md](../../docs/log/troubleshooting.md).

| File | Runs on | Purpose |
|---|---|---|
| `h96-doctor` | box (`/usr/local/sbin`) | read-only health check; every problem line names its fix (`-q`: problems only) |
| `h96-fix` | box (`/usr/local/sbin`) | `player`, `display`, `ui`, `ui-rollback`, `kmssink [alpine]`, `usb`, `wifi-bt` |
| `push-to-box.sh` | PC | install the current h96ui, video stack and this kit over ssh, then run the doctor |
| `box-packages.txt` | both | Alpine packages the playback stack needs (also used by `tools/build-rootfs.sh`) |

```bash
H96=192.168.11.149 tools/h96 h96-doctor          # what is wrong?
H96=192.168.11.149 tools/h96 h96-fix player      # e.g. a frozen picture over the menu
H96=192.168.11.149 tools/fixup/push-to-box.sh    # bring the box up to the current builds
```
Both box scripts are also in the stick inbox (`tools/make-inbox.sh`) and the rootfs image
(`tools/build-rootfs.sh`), so a reflashed box has them.
