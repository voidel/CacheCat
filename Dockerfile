FROM node:22-bookworm-slim AS web
WORKDIR /build
RUN corepack enable && corepack prepare pnpm@11.25.0 --activate
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

FROM nginxinc/nginx-unprivileged:1.28-alpine
COPY deploy/nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=web /build/dist /usr/share/nginx/html/cachecat
EXPOSE 8080
