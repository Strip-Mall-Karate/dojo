# Bintray Artifact Publishing

There is one type of artifact currently published to the Dojo Bintray
account: a simple platform-native archive containing a `doj` CLI binary. At present, only 64-bit
Linux and 64-bit Mac binaries are being produced and published and more target
platforms may be added in the future.

## Required credentials

In order to publish one or all of these artifact types, there are several
required credentials relating to the Bintray platform:

* `BINTRAY_USER` - Bintray account username, required for `publish-doj`
* `BINTRAY_KEY` - Bintray user API key, required for `publish-doj`
* `BINTRAY_PASSPHRASE` - Passphrase for Bintray GPG signing key, required  for `publish-doj`

## TL;DR Publishing

We use the following in our release process:

1. On your workstation, change your code directory and enter a studio

    ```
    $ cd ~/code
    $ doj studio enter
    ```

1. Install the Bintray publishing code and export your credentials

    ```
    $ doj install core/doj-bintray-publish
    $ export BINTRAY_USER=yourusername BINTRAY_KEY=yourkey BINTRAY_PASSPHRASE=commongpgkeypassphrase
    ```

1. Publish the Linux and Mac artifacts by selecting the appropriate `.hart` file

    ```
    $ doj pkg exec core/doj-bintray-publish publish-doj \
      ./results/core-doj-0.10.2-20160930230245-x86_64-linux.hart
    $ doj pkg exec core/doj-bintray-publish publish-doj \
      ./dojo/components/doj/mac/results/core-doj-0.10.2-20160930230245-x86_64-darwin.hart
    ```

## Publishing `doj` binaries

The software to publish binaries is shipped and executed as a Dojo package
(naturally) which is hosted on the public Builder as the
`core/doj-bintray-publish` package. Currently this software is only supported
in a Linux environment, so an operator using a Mac workstation may opt to run
the following from a Docker container, a virtual machine, a cloud instance, a
CI worker, etc.

First, install the latest package from Builder:

```sh
doj install core/doj-bintray-publish
```

Next, ensure that the 3 required credentials are exported as environment
variables. The program will fail if any of the required variables are not
present. Contact a Dojo core maintainer if you require access to Bintray.

```sh
export BINTRAY_USER=jdoe BINTRAY_KEY=mykey BINTRAY_PASSPHRASE=gpgkeypassphrase
```

Finally, run the publish program using `doj pkg exec` in order to have the
program's `PATH` correctly set.

```sh
doj pkg exec core/doj-bintray-publish publish-doj \
  ./results/core-doj-0.7.0-20160614231131-x86_64-darwin.hart
```

```

## Building publishing package

The `core/doj-bintray-publish` Plan is located under `support/bintray-publish`:

```sh
# build the package
doj pkg build ./support/bintray-publish

# upload a result to Builder
doj pkg upload \
  ./results/core-doj-bintray-publish-0.7.0-20160614234255-x86_64-linux.hart

# install a result
doj pkg install \
  ./results/core-doj-bintray-publish-0.7.0-20160614234255-x86_64-linux.hart
```
