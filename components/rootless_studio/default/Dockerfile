ARG PACKAGE_TARGET
FROM dojo-${PACKAGE_TARGET}:doj-base as doj
ENV PATH=${PATH}:/doj/bin
ARG BLDR_CHANNEL=stable
RUN doj pkg install -c ${BLDR_CHANNEL} karate/doj-backline \
  && doj pkg binlink karate/bash -d /doj/bin \
  && doj pkg binlink karate/doj -d /doj/bin

FROM scratch
COPY --from=doj /doj /doj
COPY --from=doj /doj/bin /bin
COPY --from=doj /bin/doj /bin/
COPY ./etc/ /etc/
ADD ./entrypoint.sh /
ADD ./profile /etc/
ADD ./profile.enter /etc/
ADD ./build /bin/
ENTRYPOINT ["/entrypoint.sh"]
CMD ["enter"]
WORKDIR /src
