FROM rust:1.97.1

WORKDIR /app
COPY . .

RUN cargo install --path .
CMD [ "aeroflot-snippets" ]
