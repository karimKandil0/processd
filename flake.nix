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
        };
      in
      {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            # Rust
            rustToolchain

            # C linker — required by rustc/cargo for linking
            gcc

            # Build tools
            pkg-config
            gnumake

            # Testing: boot VMs to test PID 1 behaviour
            qemu

            # Handy for reading kernel logs in tests
            utillinux
          ];

          # Lets rust-analyzer find the stdlib source
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };
      });
}
