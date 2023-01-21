{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-22.11";
    flake-parts.url = "github:hercules-ci/flake-parts";
    treefmt-nix.url = "github:numtide/treefmt-nix";
    inclusive.url="github:input-output-hk/nix-inclusive";
  };
  outputs = inputs:
    inputs.flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux" "aarch64-linux"];
      imports = [inputs.treefmt-nix.flakeModule];

      perSystem = {
        pkgs,
        lib,
        config,
        ...
      }: {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "gtk-tray";
          version = "0.0.1";

          cargoSha256 = "sha256-fDxcMHzrMP7OZ2mwcMPkhoYvUOFcg6d9g1VyYi7xgPU=";

          src = inputs.inclusive.lib.inclusive ./. [
            ./Cargo.lock
            ./Cargo.toml
            ./stray
            ./gtk-tray
          ];

          buildInputs = with pkgs; [
            gtk3
            pango
            cairo
            gtk-layer-shell
          ];

          nativeBuildInputs = with pkgs; [
            cargo
            rustc
            gcc
            pkg-config
            rust-analyzer
            rustfmt
          ];
        };

        treefmt = {
          programs.alejandra.enable = true;
          programs.rustfmt.enable = true;
          projectRootFile = "flake.nix";
        };

        formatter = pkgs.writeShellApplication {
          name = "treefmt";
          runtimeInputs = [config.treefmt.package];
          text = ''
            exec treefmt --tree-root . --config-file ${config.treefmt.build.configFile}
          '';
        };
      };
    };
}
