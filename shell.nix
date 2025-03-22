{ pkgs ? import <nixpkgs> { config = { allowUnfree = true; }; } }:

let
  cudaPackages = pkgs.cudaPackages;
  linuxPackages = pkgs.linuxPackages;
in
(pkgs.buildFHSUserEnv {
  name = "cuda-env";
  targetPkgs = pkgs: with pkgs; [
    # CUDA dependencies
    cudaPackages.cudatoolkit
    cudaPackages.cudnn
    cudaPackages.cuda_cccl
    cudaPackages.cuda_cudart
    cudaPackages.cuda_cudart.static

    # NVIDIA driver
    linuxPackages.nvidia_x11

    # Development tools
    pkg-config
    openssl
    openssl.dev
    cmake

    # C/C++ development with Clang
    clang
    clang-tools
    llvmPackages.libcxx
    llvmPackages.libclang
    llvmPackages.clang
    llvmPackages.libstdcxxClang
    llvmPackages.llvm
    stdenv.cc.cc.lib

    # Required for Rust bindings
    llvmPackages.libclang
    llvmPackages.clang
    llvmPackages.libcxx
    stdenv.cc.cc.lib

    # Standard C library headers
    glibc
    glibc.dev

    # Additional build tools
    git
    gnumake
  ];

  nativeBuildInputs = with pkgs; [
    rustPlatform.bindgenHook
  ];

  profile = ''
    # Set up CUDA environment variables
    export CUDA_PATH=${cudaPackages.cudatoolkit}
    export CUDA_HOME=${cudaPackages.cudatoolkit}
    export CUDA_LIBRARY_PATH=/usr
    export CUDNN_PATH=${cudaPackages.cudnn}

    # Set LIBCLANG_PATH for Rust bindings
    export LIBCLANG_PATH=${pkgs.llvmPackages.libclang.lib}/lib

    # Set up C++ environment variables
    export CPLUS_INCLUDE_PATH=/usr/include:/usr/lib64/clang/18/include:${pkgs.stdenv.cc.cc}/include:$CPLUS_INCLUDE_PATH
    export C_INCLUDE_PATH=/usr/include:/usr/lib64/clang/18/include:${pkgs.stdenv.cc.cc}/include:$C_INCLUDE_PATH
    export LIBRARY_PATH=/usr/lib64:${pkgs.llvmPackages.libcxx}/lib:${pkgs.stdenv.cc.cc.lib}/lib:$LIBRARY_PATH

    # Customize bash prompt to indicate FHS environment
    export PS1="\[\033[1;32m\][cuda-env]\[\033[0m\] \[\033[1;34m\]\w\[\033[0m\] $ "
  '';

  runScript = "bash --norc --noprofile";
}).env
