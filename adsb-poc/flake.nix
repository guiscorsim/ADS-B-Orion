{
  description = "ADS-B POC: readsb bench + Custom Rust DF17/18 decoder";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forEachSystem = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      devShells = forEachSystem (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            readsb
            rustc
            cargo
            rustfmt
            clippy
            ffmpeg
            hyperfine
          ];
        };
      });
    };
}
