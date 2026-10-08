# Builds Nix Software from this tree with the nixpkgs recipe, without flakes:
#   (import (builtins.fetchTarball "https://github.com/SPTApyo/nix-software/archive/main.tar.gz") { inherit pkgs; })
{
  pkgs ? import <nixpkgs> { },
}:

(pkgs.callPackage ./nix/package.nix { }).overrideAttrs (old: {
  src = pkgs.lib.cleanSourceWith {
    src = pkgs.lib.cleanSource ./.;
    filter = path: _: baseNameOf path != "target";
  };
  cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };
  # The maintainer entry only exists in nixpkgs.
  meta = old.meta // {
    maintainers = [ ];
  };
})
