{
  description = "Tern, a native GPUI client for coding agents";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (s: f nixpkgs.legacyPackages.${s});
    in {
      devShells = forAll (pkgs:
        let
          runtimeLibs = with pkgs; [
            fontconfig freetype libxkbcommon wayland vulkan-loader libGL
            libx11 libxcb libxcursor libxrandr libxi
          ];
        in {
          default = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [ pkg-config cmake perl ];
            buildInputs = runtimeLibs ++ (with pkgs; [ openssl zstd alsa-lib ]);
            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
          };
        });
    };
}
