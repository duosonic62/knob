#!/bin/sh
# Install the Knob virtual audio driver into the system HAL plug-in directory
# and restart coreaudiod so it is picked up. Requires admin privileges.
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DRIVER="$SCRIPT_DIR/../build/Release/Knob.driver"

if [ ! -d "$DRIVER" ]; then
	echo "error: Knob.driver not found at $DRIVER" >&2
	echo "build it first:" >&2
	echo "  xcodebuild -project NullAudio.xcodeproj -configuration Release \\" >&2
	echo "    CODE_SIGN_IDENTITY=\"\" CODE_SIGNING_REQUIRED=NO CODE_SIGNING_ALLOWED=NO" >&2
	exit 1
fi

echo "Installing Knob.driver to /Library/Audio/Plug-Ins/HAL/ ..."
sudo rm -rf /Library/Audio/Plug-Ins/HAL/Knob.driver
sudo cp -R "$DRIVER" /Library/Audio/Plug-Ins/HAL/
# Restart coreaudiod so it loads the driver. `launchctl kickstart` is blocked by SIP for
# Apple system services, so kill the process instead — launchd relaunches it automatically.
sudo killall coreaudiod
echo "Done. 'Knob' should now appear as an audio input/output device."
