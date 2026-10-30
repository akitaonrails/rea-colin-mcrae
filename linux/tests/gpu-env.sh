#!/bin/sh
# Tests cmr-play's GPU choice against fake sysfs trees: sh linux/tests/gpu-env.sh
set -u
# A clean environment: the player's own choice would win every case.
unset DRI_PRIME __NV_PRIME_RENDER_OFFLOAD __GLX_VENDOR_LIBRARY_NAME
HERE=$(dirname "$(readlink -f "$0")")
T=$(mktemp -d); trap 'rm -rf "$T"' EXIT
export HOME=$T CMR_PREFIX=$T/prefix
# cmr-play asks the wrapper whether NVIDIA's OpenGL library is installed.
fake_run() { [ -n "${NV_LIB:-}" ]; }
RUN=fake_run
CMR_PLAY_FUNCTIONS_ONLY=1 . "$HERE/../cmr-play"
RUN=fake_run

# gpu <tree> <card> <vendor> <driver> <boot_vga> [vram]
gpu() {
  d=$T/$1/class/drm/$2/device
  mkdir -p "$d" "$T/$1/drivers/$4" "$T/$1/class/drm/$2-DP-1"
  echo "$3" > "$d/vendor"; echo "$5" > "$d/boot_vga"
  ln -s "$T/$1/drivers/$4" "$d/driver"
  [ -n "${6:-}" ] && echo "$6" > "$d/mem_info_vram_total"
  return 0
}
fails=0
check() { # name tree expected [NV_LIB]
  got=$(CMR_SYSFS=$T/$2 NV_LIB=${4:-} gpu_env)
  if [ "$got" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$got', want '$3'"; fails=$((fails + 1)); fi
}
NV="__NV_PRIME_RENDER_OFFLOAD=1 __GLX_VENDOR_LIBRARY_NAME=nvidia __VK_LAYER_NV_optimus=NVIDIA_only"

gpu nv card0 0x10de nvidia 1
check "one NVIDIA card driving the screen" nv "$NV" yes
gpu amdnv card0 0x1002 amdgpu 0 2147483648; gpu amdnv card1 0x10de nvidia 1
check "AMD iGPU + NVIDIA on the screen" amdnv "$NV" yes
gpu laptop card0 0x8086 i915 1; gpu laptop card1 0x10de nvidia 0
check "Intel laptop screen + NVIDIA dGPU" laptop "$NV" yes
check "NVIDIA without its OpenGL library: Mesa rule" laptop "DRI_PRIME=1"
gpu inamd card0 0x8086 i915 1; gpu inamd card1 0x1002 amdgpu 0 8589934592
check "Intel iGPU + AMD dGPU" inamd "DRI_PRIME=1"
gpu apu card0 0x1002 amdgpu 1 536870912; gpu apu card1 0x1002 amdgpu 0 8589934592
check "AMD APU on the screen + AMD dGPU" apu "DRI_PRIME=1"
gpu dgpu card0 0x1002 amdgpu 1 8589934592; gpu dgpu card1 0x1002 amdgpu 0 536870912
check "AMD dGPU on the screen + AMD APU" dgpu ""
gpu intel card0 0x8086 i915 1
check "Intel only" intel ""
gpu amd card0 0x1002 amdgpu 1 8589934592
check "AMD only" amd ""
check "no GPUs listed" none ""
DRI_PRIME=0 check "the player's DRI_PRIME wins" inamd ""
[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
