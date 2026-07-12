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
    ];
  };

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  nativeBuildInputs = [
    makeWrapper
  ];


  buildInputs =
    lib.optionals stdenv.isLinux [
      wayland
      libxkbcommon
      vulkan-loader
      libGL
    ]
    ++ lib.optionals stdenv.isDarwin [
      darwin.apple_sdk.frameworks.Cocoa
      darwin.apple_sdk.frameworks.QuartzCore
      darwin.apple_sdk.frameworks.Metal
      darwin.apple_sdk.frameworks.AppKit
    ];

  postFixup = lib.optionalString stdenv.isLinux ''
    wrapProgram $out/bin/${finalAttrs.pname} \
      --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath [
        wayland
        libxkbcommon
        vulkan-loader
        libGL
      ]}
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
