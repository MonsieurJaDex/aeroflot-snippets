FROM node:22-bookworm-slim AS build
WORKDIR /app
COPY package.json package-lock.json ./
# Зависший сокет к registry обрывается по таймауту и повторяется, без audit и fund.
ENV NODE_OPTIONS=--dns-result-order=ipv4first
RUN npm ci --no-audit --no-fund \
    --fetch-timeout=60000 \
    --fetch-retries=5 \
    --fetch-retry-factor=2 \
    --fetch-retry-mintimeout=5000 \
    --fetch-retry-maxtimeout=30000
COPY . .
RUN npm run build

FROM nginx:1.27-alpine
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=build /app/dist /usr/share/nginx/html
EXPOSE 80
