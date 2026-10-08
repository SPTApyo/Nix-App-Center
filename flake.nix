{
  description = "Nix Software, a declarative app store for Nix";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};

      # The nixpkgs recipe, built from this tree (see default.nix).
      nix-software = import ./default.nix { inherit pkgs; };
    in
    {
      packages.${system} = {
        default = nix-software;
        nix-software = nix-software;
      };

      # `nix run .` opens the window; `nix run .#nix-software-cli -- search …` runs the CLI.
      apps.${system} = {
        default = {
          type = "app";
          program = "${nix-software}/bin/nix-software";
        };
        nix-software-cli = {
          type = "app";
          program = "${nix-software}/bin/nix-software-cli";
        };
      };

      devShells.${system}.default = pkgs.mkShell {
        inputsFrom = [ nix-software ];
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
    };
}
