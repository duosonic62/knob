#!/bin/sh
# Remove the Knob virtual audio driver and restart coreaudiod. Requires admin privileges.
set -eu

echo "Removing /Library/Audio/Plug-Ins/HAL/Knob.driver ..."
sudo rm -rf /Library/Audio/Plug-Ins/HAL/Knob.driver
# `launchctl kickstart` is blocked by SIP for Apple system services, so kill the process
# instead — launchd relaunches it automatically.
sudo killall coreaudiod
echo "Done. 'Knob' should no longer appear as an audio device."
