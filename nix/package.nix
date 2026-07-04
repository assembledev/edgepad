{
  lib,
  rustPlatform,
  version,
}:

rustPlatform.buildRustPackage {
  pname = "edgepad";
  inherit version;

  src = lib.cleanSourceWith {
    src = ../.;
    filter =
      path: _type:
      let
        rel = lib.removePrefix ((toString ../.) + "/") (toString path);
      in
      !(
        rel == "target"
        || lib.hasPrefix "target/" rel
        || rel == ".git"
        || lib.hasPrefix ".git/" rel
        || rel == "result"
        || lib.hasPrefix "result-" rel
      );
  };

  cargoLock.lockFile = ../Cargo.lock;

  doCheck = true;

  meta = {
    description = "Correctness-first Linux touchpad edge gesture daemon";
    homepage = "https://github.com/assembledev/edgepad";
    license = with lib.licenses; [
      asl20
      mit
    ];
    mainProgram = "edgepad";
    platforms = lib.platforms.linux;
  };
}
