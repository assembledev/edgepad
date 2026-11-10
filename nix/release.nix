{
  runCommand,
  binutils,
  edgepad,
  version,
}:

runCommand "edgepad-${version}-release" { nativeBuildInputs = [ binutils ]; } ''
  mkdir -p "$out"
  cp ${edgepad}/bin/edgepad "$out/edgepad-x86_64-unknown-linux-musl"
  if readelf -l "$out/edgepad-x86_64-unknown-linux-musl" | grep -q INTERP; then
    echo "release binary unexpectedly requires a dynamic loader" >&2
    exit 1
  fi
  test "$("$out/edgepad-x86_64-unknown-linux-musl" --version)" = "edgepad ${version}"
  cp ${../packaging/70-edgepad.rules} "$out/70-edgepad.rules"
  cp ${../packaging/edgepad.service} "$out/edgepad.service"
  cp ${../examples/edgepad.toml.example} "$out/edgepad.toml.example"
  cd "$out"
  sha256sum edgepad-x86_64-unknown-linux-musl 70-edgepad.rules edgepad.service edgepad.toml.example > checksums
''
