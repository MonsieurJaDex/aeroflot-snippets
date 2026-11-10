package main

import (
	"context"
	"os"
	"os/signal"
	"syscall"

	"github.com/MonsieurJaDex/aeroflot-snippets/backend/internal/cfg"
	"github.com/MonsieurJaDex/aeroflot-snippets/backend/internal/logging"
)

func main() {
	_, stop := signal.NotifyContext(context.Background(), syscall.SIGTERM, syscall.SIGINT)
	defer stop()

	defaultLogger := logging.NewDefaultLogger()

	config, err := cfg.LoadConfig("../.env")
	if err != nil {
		defaultLogger.Error("error loading config", "err", err.Error())
		os.Exit(1)
	}

	logger, loggerClose := logging.NewAsyncLogger(config.Debug, 1000, os.Stdout)
	defer func() {
		err := loggerClose()
		if err != nil {
			defaultLogger.Error("error closing logger", "err", err.Error())
		}
	}()

	logger.Debug("config loaded and logger initialized successfully")
}
