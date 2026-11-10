package logging

import (
	"context"
	"errors"
	"log/slog"
	"sync"
)

var ErrShutdown = errors.New("logger is shutting down")

type AsyncLoggerHandler struct {
	inner slog.Handler
	logs  chan slog.Record
	done  chan struct{}

	closeOnce sync.Once
}

func NewAsyncLoggerHandler(inner slog.Handler, bufferSize int) *AsyncLoggerHandler {
	h := &AsyncLoggerHandler{
		inner: inner,
		logs:  make(chan slog.Record, bufferSize),
		done:  make(chan struct{}),
	}

	go func() {
		defer close(h.done)

		for record := range h.logs {
			_ = h.inner.Handle(context.Background(), record)
		}
	}()

	return h
}

func (a *AsyncLoggerHandler) Enabled(ctx context.Context, level slog.Level) bool {
	return a.inner.Enabled(ctx, level)
}

func (a *AsyncLoggerHandler) Handle(ctx context.Context, record slog.Record) error {
	if err := ctx.Err(); err != nil {
		return err
	}

	cloned := record.Clone()

	select {
	case a.logs <- cloned:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

// Close инициирует graceful shutdown самого логгера.
func (a *AsyncLoggerHandler) Close() error {
	a.closeOnce.Do(func() {
		close(a.logs)
	})
	<-a.done
	return nil
}

func (a *AsyncLoggerHandler) WithAttrs(attrs []slog.Attr) slog.Handler {
	return &AsyncLoggerHandler{
		inner: a.inner.WithAttrs(attrs),
		logs:  a.logs,
		done:  a.done,
	}
}

func (a *AsyncLoggerHandler) WithGroup(name string) slog.Handler {
	return &AsyncLoggerHandler{
		inner: a.inner.WithGroup(name),
		logs:  a.logs,
		done:  a.done,
	}
}
