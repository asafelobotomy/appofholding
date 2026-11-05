#!/bin/bash
# Smoke test for appOFholding on a REAL desktop session: real home folder, real menu, real display,
# real network, real systemd user manager. It adds a copy of one of your AppImages, exercises every
# feature, and removes everything it created (it refuses to start if the library is not empty).
#
#   scripts/smoke.sh [path/to/Some.AppImage]     (default: ~/AppImages/hauler.appimage)
#
# Needs a built workspace (cargo build --release). Windows of the test app briefly appear.
# It re-runs itself in a clean `systemd-run --user` unit when started from a process tree that
# inherited NoNewPrivs (editors, agents), because FUSE-mounting an AppImage needs a setuid helper.
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd)
B=${B:-$ROOT/target/release/appofholding}
G=${G:-$ROOT/target/release/appofholding-gui}
APP=${1:-$HOME/AppImages/hauler.appimage}

if [ "${SMOKE_IN_UNIT:-0}" != 1 ] && grep -q 'NoNewPrivs:.1' /proc/self/status 2>/dev/null; then
  echo "(NoNewPrivs is set here; re-running inside a clean systemd user unit so FUSE works)"
  exec systemd-run --user --wait --collect -P -q -E SMOKE_IN_UNIT=1 -E B="$B" -E G="$G" \
    -E WAYLAND_DISPLAY -E DISPLAY -E XDG_RUNTIME_DIR -E DBUS_SESSION_BUS_ADDRESS \
    -E XDG_CURRENT_DESKTOP -E XDG_SESSION_TYPE -E HOME "$0" "$APP"
fi

[ -x "$B" ] || { echo "missing $B (run: cargo build --release)"; exit 2; }
[ -f "$APP" ] || { echo "missing test AppImage $APP"; exit 2; }
# Checked before anything else touches them: even `list` creates the database folder.
for d in ~/Applications ~/.local/share/appofholding ~/.cache/appofholding; do [ -e "$d" ] && { echo "$d already exists (an existing appOFholding library?); refusing to run"; exit 2; }; done

TMP=$(mktemp -d)
NAME=$(basename "$APP" | sed 's/\.[Aa]pp[Ii]mage$//')
ID=$(echo "$NAME" | tr 'A-Z' 'a-z' | tr -c 'a-z0-9\n' '-')
BASELINE=$(ls ~/.local/share/applications)
pass=0; fail=0
ok()  { echo "PASS  $1"; pass=$((pass+1)); }
bad() { echo "FAIL  $1   <- $2"; fail=$((fail+1)); }
check()  { d=$1; shift; if o=$("$@" 2>&1); then ok "$d"; else bad "$d" "$(echo "$o" | tail -2 | tr '\n' ' ')"; fi; }
expect() { d=$1; p=$2; shift 2; o=$("$@" 2>&1); if echo "$o" | grep -qE -- "$p"; then ok "$d"; else bad "$d" "wanted /$p/ got: $(echo "$o" | tail -3 | tr '\n' ' ' | cut -c1-200)"; fi; }
section() { echo; echo "== $1"; }
# The bracket in a pattern keeps pgrep/pkill from matching their own command line.
mounted_pids() { pgrep -f "[.]mount_$NAME" 2>/dev/null; }
sandboxed_pids() { pgrep -f '[/]app/AppRun' 2>/dev/null; }
stop_app() { pkill -f "[r]elease/appofholding run $ID" 2>/dev/null; pkill -f "[.]mount_$NAME" 2>/dev/null; sleep 3; }
cleanup() { stop_app; $B remove $ID --purge-data >/dev/null 2>&1; $B remove scale-tail --purge-data >/dev/null 2>&1; $B schedule remove >/dev/null 2>&1
  rm -rf ~/Applications ~/.local/share/appofholding ~/.cache/appofholding "$TMP"; }
trap cleanup EXIT

section "1. basics"
expect "version"                  "appofholding [0-9]+\.[0-9]+\.[0-9]+" $B --version
expect "sandbox prerequisites"    "sandboxes start:      yes"      $B sandbox check
expect "empty library"            "No apps yet"                   $B list

section "2. add a local AppImage + menu integration"
cp "$APP" "$TMP/$NAME.AppImage"
expect "add from file"            "Added "                        $B add "$TMP/$NAME.AppImage"
[ -x ~/Applications/$NAME.AppImage ] && ok "file copied to ~/Applications, executable" || bad "file copied" "missing"
D=~/.local/share/applications/appofholding-$ID.desktop
[ -f "$D" ] && ok "menu entry written" || bad "menu entry" "missing $D"
check "menu entry validates"      desktop-file-validate "$D"
ls ~/.local/share/icons/hicolor/*/apps/appofholding-$ID.* >/dev/null 2>&1 && ok "icon installed" || bad "icon" "missing"
expect "listed"                   "$ID "                          $B list
expect "adding twice is refused"  "already in the library"        $B add "$TMP/$NAME.AppImage"

section "2b. import existing AppImages (dry run only: your real files are never touched)"
BEFORE_ENTRIES=$(ls ~/.local/share/applications)
o=$($B import --dry-run 2>&1); rc=$?
[ $rc = 0 ] && ok "import --dry-run runs" || bad "import --dry-run" "exit $rc"
echo "$o" | grep -q "Dry run: nothing was changed\|Nothing selected\|No importable" && ok "it stops after showing the plan" || bad "dry run stops" "$(echo "$o" | tail -2 | tr '\n' ' ')"
echo "   ($(echo "$o" | grep -E '^Found|selected for import|No importable' | tr '\n' ' '))"
[ "$(ls ~/.local/share/applications)" = "$BEFORE_ENTRIES" ] && ok "the dry run changed no menu entries" || bad "dry run changed menu entries" ""

section "3. launch options"
check "preset + env + prefix"     $B launch set $ID --preset electron-auto --env SMOKE=1 --prefix "nice -n 5"
expect "preview shows them"       "SMOKE=1 nice -n 5 .*--ozone-platform-hint=auto" $B run --print $ID
check "extra profile"             $B launch set $ID --profile debug --preset debug
expect "profile is a menu action" "Desktop Action appofholding-debug" cat "$D"
check "menu entry still validates" desktop-file-validate "$D"
check "hook set + run"            bash -c "$B launch hook set $ID post-install 'echo ok > $TMP/hook' && $B launch hook run $ID post-install && grep -q ok $TMP/hook"
check "reset launch settings"     $B launch set $ID --reset
check "delete extra profile"      $B launch delete $ID --profile debug

section "4. launching through the menu entry (what a double-click does)"
echo "-- without a sandbox, through the AppImage's own FUSE runtime"
gtk-launch appofholding-$ID > "$TMP/menu.out" 2>&1 &
sleep 8
[ -n "$(mounted_pids)" ] && ok "app is running (FUSE-mounted by its runtime)" || bad "menu entry launch" "$(grep -v TabButton "$TMP/menu.out" | tail -2 | tr '\n' ' ')"
grep -q "Cannot mount" "$TMP/menu.out" && bad "no FUSE error" "$(grep -m1 Cannot "$TMP/menu.out")" || ok "no FUSE error"
stop_app
[ -z "$(mounted_pids)" ] && ok "app stopped" || bad "app stopped" "still running"
echo "-- inside the sandbox"
check "enable sandbox (standard)" $B sandbox set $ID --level standard
check "unpack"                    $B sandbox prepare $ID
expect "preview is a bwrap command" "bwrap .*--unshare-pid"       $B run --print $ID
gtk-launch appofholding-$ID > "$TMP/menu2.out" 2>&1 &
sleep 8
[ -n "$(sandboxed_pids)" ] && ok "app is running inside the sandbox" || bad "sandboxed launch" "$(grep -v TabButton "$TMP/menu2.out" | tail -2 | tr '\n' ' ')"
pgrep -f '[b]wrap .*--unshare-pid' >/dev/null && ok "bubblewrap process present" || bad "bwrap process" "none"
pkill -f "[r]elease/appofholding run $ID"; sleep 3
[ -z "$(sandboxed_pids)" ] && ok "app stops with its launcher" || bad "app stops with its launcher" "still running"
pgrep -x xdg-dbus-proxy >/dev/null && bad "D-Bus proxy cleaned up" "still running" || ok "D-Bus proxy cleaned up"
$B sandbox set $ID --level none >/dev/null 2>&1

section "5. updates from GitHub (real network)"
expect "add from a release tag"   "Added scale_tail"              $B add asafelobotomy/scale_tail --channel v0.4.0
check "follow latest"             $B source set scale-tail asafelobotomy/scale_tail --asset 'scale_tail-*-x86_64.AppImage' --channel latest
expect "check finds an update"    "scale-tail +update-available"  $B check scale-tail
expect "update installs it"       "updated v0.4.0 -> v"           $B update scale-tail
expect "up to date afterwards"    "already up to date"            $B update scale-tail
expect "old version is kept"      "kept +v0.4.0"                  $B versions scale-tail
expect "rollback"                 "is now version"                $B rollback scale-tail
expect "update available again"   "update-available"              $B check scale-tail
check "skip this version"         $B skip scale-tail
check "clear skip + update again" bash -c "$B skip scale-tail --clear && $B update scale-tail"

section "6. notifications and the ETag cache (real GitHub, real notification service)"
$B rollback scale-tail >/dev/null 2>&1
# A rollback deliberately marks the newer version as skipped, so nothing would be announced.
$B skip scale-tail --clear >/dev/null 2>&1
$B policy scale-tail notify >/dev/null 2>&1
dbus-monitor --session "type='method_call',interface='org.freedesktop.Notifications',member='Notify'" > "$TMP/dbus.out" 2>&1 &
MON=$!
sleep 1
for i in 1 2 3; do $B update --scheduled > "$TMP/sched$i.out" 2>&1; done
sleep 1; kill $MON 2>/dev/null
n=$(grep -c "member=Notify" "$TMP/dbus.out")
[ "$n" = 1 ] && ok "exactly one notification for three scheduled runs" || bad "one notification" "saw $n"
grep -q 'string "appofholding"' "$TMP/dbus.out" && grep -q "update available" "$TMP/dbus.out" && ok "notification has the right app name and text" || bad "notification content" "$(grep 'string' "$TMP/dbus.out" | head -2 | tr '\n' ' ')"
grep -qi "error" "$TMP/sched2.out" "$TMP/sched3.out" && bad "cached (HTTP 304) checks work" "$(cat "$TMP/sched2.out" | head -1)" || ok "repeat checks (conditional requests, HTTP 304) work"

section "7. export / apply"
check "export"                    $B export -o "$TMP/setup.toml"
expect "export names the source"  "github.com/asafelobotomy/scale_tail" cat "$TMP/setup.toml"
check "export has no machine-specific paths" bash -c "! grep -q $HOME $TMP/setup.toml"
check "apply dry run"             $B apply "$TMP/setup.toml" --dry-run
check "apply is idempotent"       $B apply "$TMP/setup.toml" --no-install

section "8. background timer (real systemd --user)"
check "schedule install"          $B schedule install --interval daily
check "timer is enabled"          systemctl --user is-enabled appofholding-update.timer
expect "timer is scheduled"       "appofholding-update.timer"     systemctl --user list-timers appofholding-update.timer --no-pager
check "run the updater service once" systemctl --user start appofholding-update.service
expect "service logged its run"   "scale-tail"                    journalctl --user -u appofholding-update.service -n 20 --no-pager
check "schedule remove"           $B schedule remove
[ ! -f ~/.config/systemd/user/appofholding-update.timer ] && ok "unit files removed" || bad "unit files removed" "still there"
systemctl --user is-enabled appofholding-update.timer >/dev/null 2>&1 && bad "timer disabled" "still enabled" || ok "timer disabled"

section "9. GUI on the real session"
QT_QPA_PLATFORM=${QT_QPA_PLATFORM:-} $G > "$TMP/gui.out" 2>&1 &
GP=$!
sleep 8
kill -0 $GP 2>/dev/null && ok "GUI started and stayed running" || bad "GUI running" "$(tail -3 "$TMP/gui.out" | tr '\n' ' ')"
grep -qiE "TypeError|ReferenceError|Cannot read|is not a function|Binding loop" "$TMP/gui.out" && bad "no QML errors" "$(grep -iE 'TypeError|ReferenceError|Cannot|Binding' "$TMP/gui.out" | head -2 | tr '\n' ' ')" || ok "no QML errors in the log"
kill $GP 2>/dev/null; wait $GP 2>/dev/null

section "10. removal and restoring your system"
expect "remove keeps private data" "private data folder was kept|Removed" $B remove $ID
check "remove scale_tail"         $B remove scale-tail
rm -rf ~/.local/share/appofholding/apps/$ID
[ ! -e "$D" ] && ok "menu entry gone" || bad "menu entry gone" "still there"
expect "library empty again"      "No apps yet"                   $B list
cleanup
[ "$(ls ~/.local/share/applications)" = "$BASELINE" ] && ok "your menu entries are exactly as before" || bad "menu entries restored" "differ"
for d in ~/Applications ~/.local/share/appofholding ~/.cache/appofholding; do [ ! -e "$d" ] && ok "removed $d" || bad "removed $d" "still exists"; done
[ -z "$(mounted_pids)$(sandboxed_pids)" ] && ok "no test processes left running" || bad "no leftovers" "processes remain"

echo
echo "RESULT: $pass passed, $fail failed"
[ "$fail" = 0 ]
