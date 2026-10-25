FROM alpine
MAINTAINER Elliott Davis <elliott@excellent.io>
ARG DOJ_VERSION=
ARG PACKAGE_TARGET
RUN set -ex \
  && apk add --no-cache --virtual .build-deps \
    ca-certificates \
    gnupg \
    libressl \
    wget \
    bash \
  \
  && cd /tmp \
  && wget https://raw.githubusercontent.com/reset/dojo/master/components/doj/install.sh \
  && bash install.sh -t ${PACKAGE_TARGET} \
  && rm -rf install.sh /doj/cache /root/.wget-hsts /root/.gnupg \
  && apk del .build-deps
