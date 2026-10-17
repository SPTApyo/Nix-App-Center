{
  description = "Nix App Center, a declarative app store for Nix";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});

      # The nixpkgs recipe, built from this tree (see default.nix).
      package = pkgs: import ./default.nix { inherit pkgs; };
    in
    {
      # programs.nix-app-center, with this flake's package.
      nixosModules.default =
        { lib, pkgs, ... }:
        {
          imports = [ ./nix/module.nix ];
          programs.nix-app-center.package = lib.mkDefault (package pkgs);
        };

      # The maintainer entry only exists in nixpkgs.
      checks = forAll (pkgs: {
        module = pkgs.testers.runNixOSTest {
          imports = [
            ./nix/test.nix
            { meta.maintainers = pkgs.lib.mkForce [ ]; }
          ];
          defaults = {
            imports = [ self.nixosModules.default ];
            meta.maintainers = pkgs.lib.mkForce [ ];
          };
        };
      });

      packages = forAll (pkgs: {
        default = package pkgs;
        nix-app-center = package pkgs;
      });

      # `nix run .` opens the window; `nix run .#nix-app-center-cli -- search …` runs the CLI.
      apps = forAll (pkgs: {
        default = {
          type = "app";
          program = "${package pkgs}/bin/nix-app-center";
        };
        nix-app-center-cli = {
          type = "app";
          program = "${package pkgs}/bin/nix-app-center-cli";
        };
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ (package pkgs) ];
          packages = with pkgs; [
            rustc
            cargo
            rust-analyzer
            clippy
            rustfmt
            just
            cargo-llvm-cov
            cargo-deny
          ];
          # cargo-llvm-cov needs the LLVM tools rustc was built with.
          LLVM_COV = "${pkgs.rustc.llvmPackages.llvm}/bin/llvm-cov";
          LLVM_PROFDATA = "${pkgs.rustc.llvmPackages.llvm}/bin/llvm-profdata";
        };
      });
    };
}
