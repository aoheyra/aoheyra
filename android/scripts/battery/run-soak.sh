#!/usr/bin/env bash
# AND-19: capture a screen-off Battery Historian run (1, 8 or 24 hours).
# Raw bugreports can contain personal data: keep them outside version control
# and commit only the distilled metrics table in android/docs/battery.md.
set -euo pipefail

hours=""
out=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        --hours)
            [ "$#" -ge 2 ] || { echo "--hours needs a value" >&2; exit 2; }
            hours="$2"; shift 2 ;;
        --out)
            [ "$#" -ge 2 ] || { echo "--out needs a value" >&2; exit 2; }
            out="$2"; shift 2 ;;
        *)
            echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

case "$hours" in
    1|8|24) ;;
    *) echo "--hours must be 1, 8 or 24" >&2; exit 2 ;;
esac
[ -n "$out" ] || { echo "--out is required" >&2; exit 2; }
command -v adb >/dev/null 2>&1 || { echo "adb not found" >&2; exit 3; }

mkdir -p "$out"
device="$(adb shell getprop ro.product.model | tr -d '\r')"
android_version="$(adb shell getprop ro.build.version.release | tr -d '\r')"

echo "Device: $device, Android $android_version"
echo "Turn the screen off, disconnect the charger and do not touch the phone for ${hours}h."
read -r -p 'Type yes when the screen is off: ' confirmation
[ "$confirmation" = "yes" ] || { echo "run cancelled" >&2; exit 4; }

adb shell dumpsys batterystats --reset
start_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
wait_seconds="$((hours * 3600))"
# SOAK_WAIT_SECONDS is only for dry-running the script; it never changes the
# recorded hours or the budget check.
if [ -n "${SOAK_WAIT_SECONDS:-}" ]; then
    wait_seconds="$SOAK_WAIT_SECONDS"
fi
sleep "$wait_seconds"
adb bugreport "$out/bugreport.zip"
end_utc="$(date -u +%Y-%m-%dT%H:%M:%SZ)"

adb shell dumpsys power > "$out/power.txt"
{
    printf '{\n'
    printf '  "device": "%s",\n' "$device"
    printf '  "androidVersion": "%s",\n' "$android_version"
    printf '  "hours": %s,\n' "$hours"
    printf '  "startUtc": "%s",\n' "$start_utc"
    printf '  "endUtc": "%s",\n' "$end_utc"
    printf '  "startBattery": null,\n'
    printf '  "endBattery": null,\n'
    printf '  "screenOnMinutes": null\n'
    printf '}\n'
} > "$out/metrics-template.json"

echo "Bugreport and metrics template in $out"
echo "Fill startBattery and endBattery from Battery Historian, set screenOnMinutes to 0, then run compute_budget.py."
