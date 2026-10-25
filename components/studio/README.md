# Dojo Studio

This is the code that creates a dojo studio for linux and windows.

The key pieces here are:

* The studio hosting script - `bin/dojo-studio`. The `.sh` sets up a linux studio and the `.ps1` sets up a windows studio.
* The studio environment "types" in `libexec`. These tweak the environment of a studio and target it for specific purposes. Most will only be interested in `doj-studio-type-default.sh`.
* The dojo plans used to create the dojo studio package.

## What determines the studio type and version used?

Different studios will run depending on OS platform, doj client version and environmental overrides.

### Linux

If you are run in a Linux host or VM, the dojo client will install the `core/doj-studio/<version>` if it is not already installed. `<version>` will match the exact version of the `doj` binary you are using. However do note that if you have a later version already installed, it will use that one instead. It will then invoke `bin/doj-studio.sh <doj studio command>` passing whatever studio command you have entered (`enter`, `build`. etc) to the bash script.

The `-t` argument will determine which studio `type` is sourced. The default is `default`.

### Experimental Windows Studio

If you have the `$env:DOJ_WINDOWS_STUDIO` variable set and are running on a Windows host, the doj client will pull down the appropriately versioned `core/doj-studio` package and invoke its `bin/doj-studio.ps1` script. Note that you must also be using a depot that hosts the windows dojo packages.

## Building a studio

You will need to build the studio and doj client packages for the same version.

### Build the doj and doj-studio packages

Navigate to the root of the dojo repo and build the doj client

```
cd /dojo
doj pkg build components/doj
```

Next build the studio

```
doj pkg build components/studio
```

The most important thing here is that you emd up with `hart` files at the same version for both `core/doj` and `core/doj-studio`.

On a linux OS, you can `doj pkg install` the `hart`s and then run the doj binary you installed with the studio command. For example:

```
/doj/pkgs/core/doj/0.22.0-dev/20170407021836/bin/doj studio enter
```
