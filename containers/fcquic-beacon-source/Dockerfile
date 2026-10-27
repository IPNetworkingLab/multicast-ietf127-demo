ARG BASE_IMAGE=base
FROM ${BASE_IMAGE}


COPY entrypoint.sh .
RUN chmod +x /entrypoint.sh

ENTRYPOINT [ "/entrypoint.sh" ]
