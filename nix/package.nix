{
  lib,
  stdenv,
  rustPlatform,
  fetchPnpmDeps,
  pnpmConfigHook,
  nodejs,
  pnpm_10,
  cmake,
  pkg-config,
  wrapGAppsHook3,
  copyDesktopItems,
  makeDesktopItem,
  webkitgtk_4_1,
  gtk3,
  glib,
  cairo,
  pango,
  gdk-pixbuf,
  libsoup_3,
  alsa-lib,
  openssl,
  libayatana-appindicator,
  ffmpeg,
  onnxruntime,
}:

let
  pname = "meetily";
  version = "0.4.1";

  cargoLock = {
    lockFile = ../Cargo.lock;
    outputHashes = {
      "cidre-0.11.3" = "sha256-6bXfAbR1E5u3+fZl5XzuRHHSZLpGR8mHJcX1fq14Dwk=";
      "ffmpeg-sidecar-2.5.0" = "sha256-/fYkQTCAogaDfJbspQEpkVTux/yJlpSnTnS/xmYNUmE=";
      "silero-0.1.0" = "sha256-1d9xXvvvgPOZ5FZsZYvvWVlfgCsDZmIKNxFZBuxc5Fo=";
    };
  };

  src = lib.cleanSourceWith {
    src = ../.;
    filter =
      path: type:
      let
        baseName = baseNameOf path;
      in
      !(
        type == "directory"
        && (
          baseName == "target"
          || baseName == "node_modules"
          || baseName == ".git"
          || baseName == "out"
        )
      )
      && !(type == "regular" && lib.hasSuffix ".out" baseName);
  };

  frontend = stdenv.mkDerivation (finalAttrs: {
    pname = "meetily-frontend";
    inherit version;
    src = ../frontend;

    nativeBuildInputs = [
      nodejs
      pnpm_10
      pnpmConfigHook
    ];

    env.CI = "true";

    pnpmDeps = fetchPnpmDeps {
      inherit (finalAttrs) pname version src;
      pnpm = pnpm_10;
      fetcherVersion = 4;
      hash = "sha256-rzLm0CKWiP4/dfgoY/drIlKW0CMtoGB+nEFFHgZ4RnM=";
    };

    postPatch = ''
      # Avoid fetching Google Fonts over the network during sandboxed build
      substituteInPlace src/app/layout.tsx \
        --replace-fail "import { Source_Sans_3 } from 'next/font/google'" "" \
        --replace-fail "const sourceSans3 = Source_Sans_3(" "const sourceSans3 = (x => ({ variable: x.variable }))("
      substituteInPlace src/app/globals.css \
        --replace-fail "--background: 0 0% 100%;" "--font-source-sans-3: 'Source Sans 3', sans-serif; --background: 0 0% 100%;"
    '';

    buildPhase = ''
      runHook preBuild
      pnpm build
      runHook postBuild
    '';

    installPhase = ''
      runHook preInstall
      cp -r out $out
      runHook postInstall
    '';
  });

  llama-helper = rustPlatform.buildRustPackage {
    pname = "llama-helper";
    inherit
      version
      src
      cargoLock
      ;
    buildAndTestSubdir = "llama-helper";

    nativeBuildInputs = [
      cmake
      pkg-config
      rustPlatform.bindgenHook
    ];
  };
in
rustPlatform.buildRustPackage (finalAttrs: {
  inherit
    pname
    version
    src
    cargoLock
    ;
  buildAndTestSubdir = "frontend/src-tauri";

  nativeBuildInputs = [
    cmake
    pkg-config
    rustPlatform.bindgenHook
    wrapGAppsHook3
    copyDesktopItems
  ];

  buildInputs = [
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

  preBuild = ''
    mkdir -p frontend/out
    cp -r ${frontend}/* frontend/out/

    mkdir -p frontend/src-tauri/binaries
    TARGET=$(rustc -vV | sed -n 's/^host: //p')
    cp ${llama-helper}/bin/llama-helper frontend/src-tauri/binaries/llama-helper-$TARGET
    cp ${ffmpeg}/bin/ffmpeg frontend/src-tauri/binaries/ffmpeg-$TARGET

    export ORT_LIB_LOCATION="${onnxruntime}"
    export ORT_PREFER_DYNAMIC_LINK=1
    export ORT_DYLIB_PATH="${onnxruntime}/lib/libonnxruntime.so"

    # Disable beforeBuildCommand in tauri.conf.json during Nix build
    substituteInPlace frontend/src-tauri/tauri.conf.json \
      --replace-fail '"beforeBuildCommand": "pnpm build"' '"beforeBuildCommand": ""'
  '';

  postInstall = ''
    install -Dm755 ${llama-helper}/bin/llama-helper $out/bin/llama-helper
    install -Dm644 frontend/src-tauri/icons/icon.png $out/share/icons/hicolor/512x512/apps/meetily.png
    install -Dm644 frontend/src-tauri/icons/128x128.png $out/share/icons/hicolor/128x128/apps/meetily.png
    install -Dm644 frontend/src-tauri/icons/32x32.png $out/share/icons/hicolor/32x32/apps/meetily.png

    install -d $out/lib/meetily/templates
    install -m644 frontend/src-tauri/templates/*.json $out/lib/meetily/templates/
  '';

  preFixup = ''
    gappsWrapperArgs+=(
      --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath [ libayatana-appindicator onnxruntime ]}"
      --prefix PATH : "${lib.makeBinPath [ ffmpeg ]}"
      --set-default FFMPEG_PATH "${ffmpeg}/bin/ffmpeg"
      --set-default MEETILY_LLAMA_HELPER "$out/bin/llama-helper"
      --set-default ORT_DYLIB_PATH "${onnxruntime}/lib/libonnxruntime.so"
      --set-default RESOURCE_DIR "$out/lib/meetily"
    )
  '';

  desktopItems = [
    (makeDesktopItem {
      name = "meetily";
      exec = "meetily";
      icon = "meetily";
      comment = "Privacy-first AI meeting assistant";
      desktopName = "Meetily";
      genericName = "AI Meeting Assistant";
      categories = [
        "AudioVideo"
        "Audio"
        "Utility"
      ];
    })
  ];

  passthru = {
    inherit frontend llama-helper;
  };

  meta = {
    description = "Privacy-first AI meeting assistant with local transcription and summarization";
    homepage = "https://meetily.ai";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "meetily";
  };
})
