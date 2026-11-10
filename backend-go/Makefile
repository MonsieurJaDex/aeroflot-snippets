.PHONY: run build

all: run

build:
	@go build -race -o build ./cmd/api

run:
	@go run -race ./cmd/api

clean:
	@rm -rf ./build/*