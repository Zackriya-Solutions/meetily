{
  description = "Meetily - Privacy-first AI meeting assistant with local transcription and summarization";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    let
      supportedSystems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
    in
    flake-utils.lib.eachSystem supportedSystems (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };
      in
      rec {
        packages = rec {
          meetily = pkgs.callPackage ./nix/package.nix { };
          llama-helper = packages.meetily.llama-helper;
          frontend = packages.meetily.frontend;
          default = meetily;
        };

        apps.default = {
          type = "app";
          program = "${packages.default}/bin/meetily";
        };

        devShells.default = pkgs.mkShell {
          name = "meetily-dev";

          nativeBuildInputs = with pkgs; [
            cargo
            rustc
            rustPlatform.bindgenHook
            cmake
            pkg-config
            nodejs
            pnpm_10
            wrapGAppsHook3
          ];

          buildInputs = with pkgs; [
            webkitgtk_4_1
            gtk3
            glib
            cairo
            pango
            gdk-pixbuf
            libsoup_3
            alsa-lib
            openssl
            libayatana-appindicator
            ffmpeg
            onnxruntime
          ];

          shellHook = ''
            export LD_LIBRARY_PATH="${
              pkgs.lib.makeLibraryPath (
                with pkgs;
                [
                  libayatana-appindicator
                  onnxruntime
                  alsa-lib
                  webkitgtk_4_1
                  gtk3
                  glib
                ]
              )
            }:$LD_LIBRARY_PATH"
            export FFMPEG_PATH="${pkgs.ffmpeg}/bin/ffmpeg"
            export ORT_LIB_LOCATION="${pkgs.onnxruntime}"
            export ORT_PREFER_DYNAMIC_LINK=1
            export ORT_DYLIB_PATH="${pkgs.onnxruntime}/lib/libonnxruntime.so"

            echo "🚀 Meetily NixOS Development Shell"
            echo "   Rust: $(rustc --version)"
            echo "   Node: $(node --version)"
            echo "   pnpm: $(pnpm --version)"
          '';
        };
      }
    )
    // {
      nixosModules = {
        meetily = import ./nix/module.nix;
        default = self.nixosModules.meetily;
      };

      overlays.default = final: prev: {
        meetily = final.callPackage ./nix/package.nix { };
      };
    };
}
