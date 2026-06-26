{
  description = "processd — declarative reconciliation-driven init system";

  inputs = {
    nixpkgs.url     = "nixpkgs/nixos-26.05";
    rust-overlay    = {
      url    = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "clippy" "rustfmt" "rust-analyzer" ];
          targets    = [ "x86_64-unknown-linux-musl" ];
        };
      in
      {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            # Rust
            rustToolchain

            # C linker — required by rustc/cargo for linking
            gcc

            # musl cross-compiler for static binaries
            pkgsMusl.stdenv.cc

            # Build tools
            pkg-config
            gnumake

            # Testing: boot VMs to test PID 1 behaviour
            qemu

            # For building minimal rootfs images
            pkgsStatic.busybox
            cpio
            util-linux
          ];

          # Lets rust-analyzer find the stdlib source
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";

          shellHook = ''
            alias build-musl="cargo build --target x86_64-unknown-linux-musl"

            alias build-rootfs='
              rm -rf rootfs &&
              mkdir -p rootfs/{bin,proc,sys,dev,etc/processd} &&
              install -m755 target/x86_64-unknown-linux-musl/debug/processd rootfs/init &&
              install -m755 ${pkgs.pkgsStatic.busybox}/bin/busybox rootfs/bin/busybox &&
              install -m644 test/system.toml rootfs/etc/processd/system.toml &&
              ln -sf busybox rootfs/bin/sh &&
              ln -sf busybox rootfs/bin/sleep &&
              ln -sf busybox rootfs/bin/ps &&
              (cd rootfs && find . | cpio -oH newc | gzip > ../initramfs.cpio.gz) &&
              echo "rootfs built"
            '

            alias boot-vm="qemu-system-x86_64 \
              -kernel /run/current-system/kernel \
              -initrd initramfs.cpio.gz \
              -append \"console=ttyS0 init=/init\" \
              -nographic \
              -m 256M"
          '';

          # Tell cargo to use the musl cross-compiler for the musl target
          CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = "${pkgs.pkgsMusl.stdenv.cc}/bin/cc";
        };
      });
}
