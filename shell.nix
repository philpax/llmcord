{ pkgs ? import <nixpkgs> { config = { allowUnfree = true; }; } }:

let
  cudaPackages = pkgs.cudaPackages;
  linuxPackages = pkgs.linuxPackages;
in
pkgs.mkShell {
  buildInputs = with pkgs; [
    # CUDA dependencies with full runtime
    cudaPackages.cudatoolkit
    cudaPackages.cudnn
    cudaPackages.cudnn.lib
    cudaPackages.cudnn.dev
    cudaPackages.cuda_cccl

    # NVIDIA driver
    linuxPackages.nvidia_x11

    # Other potential dependencies
    pkg-config
    openssl
    openssl.dev
  ];

  # Set environment variables for CUDA
  shellHook = ''
    export CUDA_PATH=${cudaPackages.cudatoolkit}
    export CUDNN_PATH=${cudaPackages.cudnn}
    export CUDNN_LIB=${cudaPackages.cudnn.dev}
    export LD_LIBRARY_PATH=${cudaPackages.cudatoolkit}/lib:${cudaPackages.cudnn.lib}:${linuxPackages.nvidia_x11}/lib:$LD_LIBRARY_PATH
    export LIBRARY_PATH=${cudaPackages.cudatoolkit}/lib:${cudaPackages.cudnn.lib}:${linuxPackages.nvidia_x11}/lib:$LIBRARY_PATH

    # Ensure CUDA driver is properly detected
    export CUDA_VISIBLE_DEVICES=0
  '';
}
