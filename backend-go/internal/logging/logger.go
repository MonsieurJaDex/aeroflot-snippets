package logging

import (
	"io"
	"log/slog"
	"os"
)

func NewDefaultLogger() *slog.Logger {
	jsonHandler := slog.NewJSONHandler(os.Stdout, &slog.HandlerOptions{
		AddSource:   true,
		Level:       slog.LevelError,
		ReplaceAttr: nil,
	})

	logger := slog.New(jsonHandler)

	return logger
}

func NewAsyncLogger(debug bool, bufferSize int, w io.Writer) (*slog.Logger, func() error) {
	var level slog.Level
	if debug {
		level = slog.LevelDebug
	} else {
		level = slog.LevelInfo
	}

	var handler slog.Handler
	if !debug {
		handler = slog.NewJSONHandler(w, &slog.HandlerOptions{
			AddSource:   false,
			Level:       level,
			ReplaceAttr: nil,
		})
	} else {
		handler = slog.NewTextHandler(w, &slog.HandlerOptions{
			AddSource:   false,
			Level:       level,
			ReplaceAttr: nil,
		})
	}

	asyncHandler := NewAsyncLoggerHandler(handler, bufferSize)

	return slog.New(asyncHandler), asyncHandler.Close
}
