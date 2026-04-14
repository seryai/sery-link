#!/usr/bin/env python3
"""
Build script for the MarkItDown sidecar binary using PyInstaller.

This creates a standalone executable that bundles Python + MarkItDown + all dependencies.
No external Python installation required.

Usage:
    python build.py

Output:
    dist/markitdown-sidecar (or markitdown-sidecar.exe on Windows)
"""
import sys
import os
import platform
import subprocess
from pathlib import Path

def main():
    """Build the sidecar binary."""

    # Ensure PyInstaller is installed
    try:
        import PyInstaller
    except ImportError:
        print("PyInstaller not found. Installing...")
        subprocess.check_call([sys.executable, "-m", "pip", "install", "pyinstaller"])

    # Ensure MarkItDown is installed
    subprocess.check_call([sys.executable, "-m", "pip", "install", "-r", "requirements.txt"])

    # Determine output name based on platform
    system = platform.system()
    if system == "Windows":
        binary_name = "markitdown-sidecar.exe"
    else:
        binary_name = "markitdown-sidecar"

    # PyInstaller command
    cmd = [
        sys.executable, "-m", "PyInstaller",
        "--onefile",                    # Single executable
        "--name", binary_name.replace(".exe", ""),  # Output name
        "--clean",                      # Clean cache
        "--noconfirm",                  # Overwrite without asking
        "markitdown_worker.py"
    ]

    print(f"Building sidecar for {system}...")
    print(f"Command: {' '.join(cmd)}")

    subprocess.check_call(cmd)

    # Verify the binary was created
    dist_dir = Path("dist")
    binary_path = dist_dir / binary_name

    if binary_path.exists():
        size_mb = binary_path.stat().st_size / (1024 * 1024)
        print(f"\n✅ Sidecar built successfully!")
        print(f"   Location: {binary_path}")
        print(f"   Size: {size_mb:.1f} MB")

        # Create platform-specific copy for Tauri
        # Tauri expects binaries named like: sidecar-aarch64-apple-darwin
        import platform as plat
        machine = plat.machine().lower()

        # Map Python platform to Rust target triple
        target_triple = None
        if system == "Darwin":
            if machine == "arm64":
                target_triple = "aarch64-apple-darwin"
            else:
                target_triple = "x86_64-apple-darwin"
        elif system == "Windows":
            if machine == "amd64" or machine == "x86_64":
                target_triple = "x86_64-pc-windows-msvc"
            else:
                target_triple = "i686-pc-windows-msvc"
        elif system == "Linux":
            if machine == "x86_64":
                target_triple = "x86_64-unknown-linux-gnu"
            elif machine.startswith("arm") or machine.startswith("aarch"):
                target_triple = "aarch64-unknown-linux-gnu"

        if target_triple:
            platform_binary_name = binary_name.replace(".exe", "") + f"-{target_triple}"
            if system == "Windows":
                platform_binary_name += ".exe"
            platform_binary_path = dist_dir / platform_binary_name

            # Copy the binary with platform-specific name
            import shutil
            shutil.copy2(binary_path, platform_binary_path)
            print(f"\n📦 Created Tauri-compatible binary:")
            print(f"   {platform_binary_path}")

        print(f"\nTest it:")
        print(f'   echo "/path/to/document.docx" | {binary_path}')
    else:
        print(f"\n❌ Build failed - binary not found at {binary_path}")
        sys.exit(1)


if __name__ == "__main__":
    main()
