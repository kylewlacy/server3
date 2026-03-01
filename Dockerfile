FROM ubuntu:24.04
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
ARG TARGETARCH
ARG TARGETOS
COPY ./artifacts/$TARGETARCH-$TARGETOS/server3 /usr/local/bin/server3
RUN chmod +x /usr/local/bin/server3
EXPOSE 3000/tcp
EXPOSE 3001/tcp
CMD ["server3"]
