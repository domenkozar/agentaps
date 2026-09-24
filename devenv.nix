{ pkgs, ... }: {
  packages = with pkgs; [
    pkg-config
    nodejs
    xorg.libxcb
    libxkbcommon
    wayland
    vulkan-loader
  ];

  languages.rust = {
    enable = true;
    channel = "stable";
    version = "1.97.1";
  };

  env.LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath [
    pkgs.xorg.libxcb
    pkgs.libxkbcommon
    pkgs.wayland
    pkgs.vulkan-loader
  ];
}
