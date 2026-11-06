{
  pkgs ? import <nixpkgs> { },
}:

pkgs.callPackage ./nix/package.nix { }
