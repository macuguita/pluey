{
  rustPlatform,
  lib,
  stdenv,
  makeWrapper,
  wayland,
  libxkbcommon,
  vulkan-loader,
  libGL,
  darwin,
  imagemagick,
  libicns,
  copyDesktopItems,
  makeDesktopItem,
}:
let
  fs = lib.fileset;
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = finalAttrs.passthru.cargoToml.package.name;
  inherit (finalAttrs.passthru.cargoToml.package) version;

  src = fs.toSource {
    root = ../.;
    fileset = fs.unions [
      ../src
      ../Cargo.lock
      ../Cargo.toml
      ../package
    ];
  };

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  nativeBuildInputs = [
    makeWrapper
    copyDesktopItems
    imagemagick
  ]
  ++ lib.optionals stdenv.isDarwin [
    libicns
  ];

  buildInputs = lib.optionals stdenv.isLinux [
    wayland
    libxkbcommon
    vulkan-loader
    libGL
  ];

  postFixup = lib.optionalString stdenv.isLinux ''
    wrapProgram $out/bin/${finalAttrs.pname} \
      --prefix LD_LIBRARY_PATH : ${
        lib.makeLibraryPath [
          wayland
          libxkbcommon
          vulkan-loader
          libGL
        ]
      }
  '';

  desktopItems = lib.optionals stdenv.isLinux (
    lib.singleton (makeDesktopItem {
      name = "com.macuguita.Pluey";
      desktopName = "Pluey";
      exec = "pluey %F";
      icon = "pluey";
      comment = "Fast image viewer";
      categories = [
        "Graphics"
        "Viewer"
      ];
      mimeTypes = [
        "image/jpeg"
        "image/png"
        "image/webp"
        "image/gif"
        "image/bmp"
        "image/tiff"
        "image/x-portable-pixmap"
        "image/x-portable-graymap"
        "image/x-portable-bitmap"
        "image/x-portable-anymap"
        "image/x-tga"
        "image/x-xbitmap"
        "image/x-xpixmap"
        "image/avif"
        "image/heic"
        "image/heif"
      ];
    })
  );

  postInstall =
    lib.optionalString stdenv.isLinux ''
      for size in 16 24 32 48 64 128 256; do
        geometry="$size"x"$size"
        mkdir -p "$out/share/icons/hicolor/$geometry/apps"
        magick package/pluey.png -resize "$geometry" \
          "$out/share/icons/hicolor/$geometry/apps/pluey.png"
      done
    ''
    + lib.optionalString stdenv.isDarwin ''
      app="$out/Applications/Pluey.app"
      mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

      cp "$out/bin/${finalAttrs.pname}" "$app/Contents/MacOS/pluey"

      substitute package/mac/Info.plist.in "$app/Contents/Info.plist" \
        --subst-var-by version "${finalAttrs.version}"

      iconDir="$TMPDIR/icons"
      mkdir -p "$iconDir"
      for size in 16 32 48 128 256 512; do
        magick package/pluey.png -resize "''${size}x''${size}" "$iconDir/icon_$size.png"
      done
      png2icns "$app/Contents/Resources/AppIcon.icns" "$iconDir"/icon_*.png
    '';

  passthru = {
    cargoToml = lib.importTOML ../Cargo.toml;
  };

  meta = {
    description = "Simple image viewer";
    homepage = "https://github.com/macuguita/pluey";
    mainProgram = finalAttrs.pname;
    license = lib.licenses.eupl12;
    sourceProvenance = with lib.sourceTypes; [
      fromSource
    ];
  };
})
