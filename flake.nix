{
  description = "Onyx — app store déclaratif pour Nix";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};

      # Dépendances système partagées entre la dérivation et le devShell : une seule
      # source de vérité (le shell reproduit exactement l'environnement de build).
      nativeBuildInputs = with pkgs; [
        pkg-config
        wrapGAppsHook4
      ];
      buildInputs = with pkgs; [
        gtk4
        libadwaita
        glib
        gsettings-desktop-schemas
      ];

      onyx = pkgs.rustPlatform.buildRustPackage {
        pname = "onyx";
        version = "0.1.0";
        src = self; # les fichiers gitignored (target/, result, .direnv) sont exclus
        cargoLock.lockFile = ./Cargo.lock;
        inherit nativeBuildInputs buildInputs;

        # Les tests GTK nécessitent un display absent du sandbox Nix ; la CI les couvre.
        doCheck = false;

        # `cargo build` produit `onyx` (GUI) et `onyx-cli` dans $out/bin. On ajoute le
        # `.desktop` et l'icône pour que l'app apparaisse dans les menus / logithèques.
        postInstall = ''
          install -Dm644 crates/gui/data/dev.onyx.Onyx.desktop \
            $out/share/applications/dev.onyx.Onyx.desktop
          install -Dm644 crates/gui/data/dev.onyx.Onyx.metainfo.xml \
            $out/share/metainfo/dev.onyx.Onyx.metainfo.xml
          install -Dm644 crates/gui/data/icons/hicolor/scalable/apps/dev.onyx.Onyx.svg \
            $out/share/icons/hicolor/scalable/apps/dev.onyx.Onyx.svg
        '';

        meta = with pkgs.lib; {
          description = "App store déclaratif pour Nix (try-first : nix run, puis keep)";
          license = licenses.gpl3Plus;
          mainProgram = "onyx";
          platforms = platforms.linux;
        };
      };
    in
    {
      packages.${system} = {
        default = onyx;
        onyx = onyx;
      };

      # `nix run .` lance la fenêtre ; `nix run .#onyx-cli -- search …` le banc d'essai.
      apps.${system} = {
        default = {
          type = "app";
          program = "${onyx}/bin/onyx";
        };
        onyx-cli = {
          type = "app";
          program = "${onyx}/bin/onyx-cli";
        };
      };

      devShells.${system}.default = pkgs.mkShell {
        inherit nativeBuildInputs buildInputs;
        packages = with pkgs; [
          rustc
          cargo
          rust-analyzer
          clippy
          rustfmt
        ];
      };
    };
}
