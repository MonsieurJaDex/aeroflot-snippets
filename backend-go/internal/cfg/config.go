package cfg

import (
	"fmt"
	"os"
	"strconv"

	"github.com/joho/godotenv"
)

type Config struct {
	Debug bool `json:"debug"`
}

func LoadConfig(path string) (*Config, error) {
	if err := godotenv.Load(path); err != nil {
		return nil, fmt.Errorf("error loading .env file: %v", err)
	}

	debug, err := strconv.ParseBool(os.Getenv("DEBUG"))
	if err != nil {
		return nil, fmt.Errorf("error parsing DEBUG env var: %v", err)
	}

	return &Config{
		debug,
	}, nil
}
