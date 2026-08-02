#!/bin/bash
# Build environment for the LocalRAG1 project on this Windows box.
# Source this file before running cargo commands.
#   $ source ./build-env.sh
#   $ cargo build

export VSDIR='C:\Program Files\Microsoft Visual Studio\18\Professional\SDK\ScopeCppSDK\vc15'

# Add MSVC bin to PATH FIRST so MSVC's link.exe beats MSYS's.
export PATH="/c/Program Files/Microsoft Visual Studio/18/Professional/SDK/ScopeCppSDK/vc15/VC/bin:/c/Users/SujitKumarRaul/.cargo/bin:$PATH"

# INCLUDE: crt, ucrt, sdk, vc — order matters
export INCLUDE='C:\Program Files\Microsoft Visual Studio\18\Professional\SDK\ScopeCppSDK\vc15\VC\include;C:\Program Files\Microsoft Visual Studio\18\Professional\SDK\ScopeCppSDK\vc15\SDK\include\ucrt;C:\Program Files\Microsoft Visual Studio\18\Professional\SDK\ScopeCppSDK\vc15\SDK\include\shared;C:\Program Files\Microsoft Visual Studio\18\Professional\SDK\ScopeCppSDK\vc15\SDK\include\um;C:\Program Files\Microsoft Visual Studio\18\Professional\SDK\ScopeCppSDK\vc15\SDK\include\winrt'

# LIB: linker search paths
export LIB='C:\Program Files\Microsoft Visual Studio\18\Professional\SDK\ScopeCppSDK\vc15\VC\lib;C:\Program Files\Microsoft Visual Studio\18\Professional\SDK\ScopeCppSDK\vc15\SDK\lib'

# Force cargo to use MSVC's linker
export CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER='C:\Program Files\Microsoft Visual Studio\18\Professional\SDK\ScopeCppSDK\vc15\VC\bin\link.exe'
