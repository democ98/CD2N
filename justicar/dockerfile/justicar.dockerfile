FROM --platform=linux/amd64 ubuntu:22.04 AS builder

ARG TZ="Etc/UTC"
ARG RUST_TOOLCHAIN="1.73.0"

WORKDIR /root
ENV HOME=/root

RUN DEBIAN_FRONTEND="noninteractive" apt-get update && \
    DEBIAN_FRONTEND="noninteractive" apt-get upgrade -y && \
    DEBIAN_FRONTEND="noninteractive" apt-get install -y apt-utils apt-transport-https software-properties-common readline-common curl vim wget gnupg gnupg2 gnupg-agent ca-certificates cmake pkg-config libssl-dev git build-essential llvm clang libclang-dev rsync libboost-all-dev libssl-dev zlib1g-dev miniupnpc

RUN curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain="${RUST_TOOLCHAIN}" && \
    $HOME/.cargo/bin/rustup target add wasm32-unknown-unknown --toolchain "${RUST_TOOLCHAIN}"

RUN curl -fsSLo /usr/share/keyrings/intel-sgx-deb.asc https://download.01.org/intel-sgx/sgx_repo/ubuntu/intel-sgx-deb.key && \
    echo "deb [arch=amd64 signed-by=/usr/share/keyrings/intel-sgx-deb.asc] https://download.01.org/intel-sgx/sgx_repo/ubuntu $(lsb_release -sc) main" | tee /etc/apt/sources.list.d/intel-sgx.list

RUN curl -fsSLo /usr/share/keyrings/gramine-keyring.gpg https://packages.gramineproject.io/gramine-keyring.gpg && \
    echo "deb [arch=amd64 signed-by=/usr/share/keyrings/gramine-keyring.gpg] https://packages.gramineproject.io/ $(lsb_release -sc) main" | tee /etc/apt/sources.list.d/gramine.list

RUN DEBIAN_FRONTEND="noninteractive" apt-get update && \
    DEBIAN_FRONTEND="noninteractive" apt-get install -y \
        libsgx-headers \
        libsgx-ae-epid \
        libsgx-ae-le \
        libsgx-ae-pce \
        libsgx-aesm-ecdsa-plugin \
        libsgx-aesm-epid-plugin \
        libsgx-aesm-launch-plugin \
        libsgx-aesm-pce-plugin \
        libsgx-aesm-quote-ex-plugin \
        libsgx-enclave-common \
        libsgx-enclave-common-dev \
        libsgx-epid \
        libsgx-epid-dev \
        libsgx-launch \
        libsgx-launch-dev \
        libsgx-quote-ex \
        libsgx-quote-ex-dev \
        libsgx-uae-service \
        libsgx-urts \
        libsgx-ae-qe3 \
        libsgx-pce-logic \
        libsgx-qe3-logic \
        libsgx-ra-network \
        libsgx-ra-uefi \
        libsgx-dcap-default-qpl \
        libsgx-dcap-default-qpl-dev \
        libsgx-dcap-quote-verify \
        libsgx-dcap-quote-verify-dev \
        libsgx-dcap-ql \
        libsgx-dcap-ql-dev \
        sgx-aesm-service \
        gramine && \
    apt-get clean -y

RUN DEBIAN_FRONTEND="noninteractive" apt-get install -y rsync unzip lsb-release debhelper gettext cmake reprepro autoconf automake bison build-essential curl dpkg-dev expect flex gcc gdb git git-core gnupg kmod libboost-system-dev libboost-thread-dev libcurl4-openssl-dev libiptcdata0-dev libjsoncpp-dev liblog4cpp5-dev libprotobuf-dev libssl-dev libtool libxml2-dev uuid-dev ocaml ocamlbuild pkg-config protobuf-compiler gawk nasm ninja-build python3 python3-pip python3-click python3-jinja2 texinfo llvm clang libclang-dev && \
    DEBIAN_FRONTEND="noninteractive" apt-get clean -y

RUN PATH="$PATH:$HOME/.cargo/bin" cargo install sgxs-tools
ARG APP_NAME="justicar"
RUN mkdir -p $HOME/${APP_NAME}

ARG APP_NAME="justicar"

ARG RA_TYPE="dcap"
ARG SGX_SIGNER_KEY="enclave-key.pem"

ARG APP_DEPLOYMENT_DIR="/opt/justicar"
COPY priv.build_stage .priv
COPY gramine-build $HOME/${APP_NAME}/gramine-build
COPY sgx-attestation $HOME/${APP_NAME}/sgx-attestation
COPY src $HOME/${APP_NAME}/src
COPY Cargo.toml Cargo.lock rust-toolchain.toml dockerfile/start_justicar.sh $HOME/${APP_NAME}/

RUN cd $HOME/${APP_NAME}/gramine-build && \
    PATH="$PATH:$HOME/.cargo/bin" make dist PREFIX="${APP_DEPLOYMENT_DIR}" && \
    PATH="$PATH:$HOME/.cargo/bin" make clean && \
    rm -rf $HOME/.priv/*

# ====

FROM --platform=linux/amd64 ubuntu:22.04

ARG TZ="Etc/UTC"

RUN DEBIAN_FRONTEND="noninteractive" apt-get update && \
    DEBIAN_FRONTEND="noninteractive" apt-get upgrade -y && \
    DEBIAN_FRONTEND="noninteractive" apt-get install -y apt-utils apt-transport-https software-properties-common readline-common curl vim wget gnupg gnupg2 gnupg-agent ca-certificates git unzip tini

RUN curl -fsSLo /usr/share/keyrings/intel-sgx-deb.asc https://download.01.org/intel-sgx/sgx_repo/ubuntu/intel-sgx-deb.key && \
    echo "deb [arch=amd64 signed-by=/usr/share/keyrings/intel-sgx-deb.asc] https://download.01.org/intel-sgx/sgx_repo/ubuntu $(lsb_release -sc) main" | tee /etc/apt/sources.list.d/intel-sgx.list

RUN curl -fsSLo /usr/share/keyrings/gramine-keyring.gpg https://packages.gramineproject.io/gramine-keyring.gpg && \
    echo "deb [arch=amd64 signed-by=/usr/share/keyrings/gramine-keyring.gpg] https://packages.gramineproject.io/ $(lsb_release -sc) main" | tee /etc/apt/sources.list.d/gramine.list

RUN DEBIAN_FRONTEND="noninteractive" apt-get update && \
    DEBIAN_FRONTEND="noninteractive" apt-get install -y \
        python3 python3-pip \
        libsgx-headers \
        libsgx-ae-epid \
        libsgx-ae-le \
        libsgx-ae-pce \
        libsgx-aesm-ecdsa-plugin \
        libsgx-aesm-epid-plugin \
        libsgx-aesm-launch-plugin \
        libsgx-aesm-pce-plugin \
        libsgx-aesm-quote-ex-plugin \
        libsgx-enclave-common \
        libsgx-enclave-common-dev \
        libsgx-epid \
        libsgx-epid-dev \
        libsgx-launch \
        libsgx-launch-dev \
        libsgx-quote-ex \
        libsgx-quote-ex-dev \
        libsgx-uae-service \
        libsgx-urts \
        libsgx-ae-qe3 \
        libsgx-pce-logic \
        libsgx-qe3-logic \
        libsgx-ra-network \
        libsgx-ra-uefi \
        libsgx-dcap-default-qpl \
        libsgx-dcap-quote-verify \
        libsgx-dcap-ql \
        sgx-aesm-service \
        gramine && \
    DEBIAN_FRONTEND="noninteractive" apt-get clean -y
RUN echo "Gramine SGX Version:" && gramine-sgx --version
COPY --from=builder /root/.cargo/bin/sgx-detect /usr/local/bin

ARG APP_DEPLOYMENT_DIR="/opt/justicar"

COPY --from=builder ${APP_DEPLOYMENT_DIR} ${APP_DEPLOYMENT_DIR}
ADD dockerfile/start_justicar.sh ${APP_DEPLOYMENT_DIR}/start_justicar.sh

ADD dockerfile/conf /opt/conf

WORKDIR ${APP_DEPLOYMENT_DIR}

ENV SGX=1
ENV SKIP_AESMD=0
ENV SLEEP_BEFORE_START=6
ENV RUST_LOG="info"

ENTRYPOINT ["/usr/bin/tini", "--"]

CMD ["/bin/bash", "start_justicar.sh"]