{
  pkgs ? import <nixpkgs> { },
}:

let
  flake = import ./flake.nix;
  system = pkgs.stdenv.hostPlatform.system;
in
pkgs.mkShell {
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
}
