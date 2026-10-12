# Genkan's image: one static binary and Mozilla's CA bundle (for upstream identity providers and
# mail servers); no shell, no package manager. kube-apiserver is trusted through the service
# account's CA only. Runs as a non-root user.
# Built by scripts/build-image.sh.
FROM scratch
COPY --chmod=0555 genkan /genkan
COPY --chmod=0444 ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
USER 65532:65532
ENTRYPOINT ["/genkan"]
