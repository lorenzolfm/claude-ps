{
  description = "ps for Claude Code, joined to the zellij pane each agent runs in";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
  };

  outputs = inputs @ {
    flake-parts,
    nixpkgs,
    rust-overlay,
    crane,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      # Linux only, unlike the other repos here. The join this tool performs reads
      # /proc/<pid>/environ and /proc/<pid>/stat, and there is no procfs on darwin. It
      # would compile there and then report no agents at all, which is worse than not
      # being offered.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      perSystem = {
        system,
        pkgs,
        ...
      }: let
        craneLib = (crane.mkLib pkgs).overrideToolchain (p:
          p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml);

        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;
        };

        # Build only the dependencies, so CI can cache that work.
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        claude-ps = craneLib.buildPackage (commonArgs
          // {
            inherit cargoArtifacts;

            pname = "claude-ps";

            # The test gate runs the tests. Do not run them two times.
            doCheck = false;

            meta = {
              description = "ps for Claude Code, joined to the zellij pane each agent runs in";
              homepage = "https://github.com/lorenzolfm/claude-ps";
              license = pkgs.lib.licenses.mit;
              mainProgram = "claude-ps";
              platforms = pkgs.lib.platforms.linux;
            };
          });

        # One derivation for each gate. CI builds them in parallel, and
        # `nix flake check` runs all of them.
        gates = {
          inherit claude-ps;

          # Each gate is a separate derivation. A lint failure therefore stops
          # CI, but it does not stop a user who only wants to build the crate.
          claude-ps-clippy = craneLib.cargoClippy (commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });

          claude-ps-test = craneLib.cargoNextest (commonArgs
            // {
              inherit cargoArtifacts;
              partitions = 1;
              partitionType = "count";
            });

          claude-ps-fmt = craneLib.cargoFmt {inherit src;};
        };
      in {
        _module.args.pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };

        checks = gates;

        # Give each gate a package name, so CI can build one gate with
        # `nix build .#<gate>`.
        packages =
          gates
          // {
            default = claude-ps;
          };

        apps.default = {
          type = "app";
          program = "${pkgs.lib.getExe claude-ps}";
        };

        devShells.default = craneLib.devShell {
          packages = with pkgs; [
            cargo-nextest
          ];

          shellHook = ''
            echo "  Rust: $(rustc --version)"
          '';
        };

        formatter = pkgs.alejandra;
      };
    };
}
