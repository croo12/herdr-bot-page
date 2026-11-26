#!/bin/sh
set -eu

root="${HERDR_PLUGIN_ROOT:-$(cd "$(dirname "$0")/../.." && pwd)}"

for app in "/Applications/herdr-bot.app" "$HOME/Applications/herdr-bot.app" "$root/desktop/target/release/bundle/macos/herdr-bot.app"; do
  if [ -d "$app" ]; then
    exec open -a "$app"
  fi
done

echo "herdr-bot is not built yet. Reinstall the plugin: herdr plugin install croo12/herdr-bot-page" >&2
exit 1
