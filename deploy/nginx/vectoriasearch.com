# Vectoria — nginx configuration for vectoriasearch.com
# Managed by deploy/deploy.sh — do not edit manually on the server.
#
# Domains:
#   vectoriasearch.com / www  →  marketing website (static files)
#   demo.vectoriasearch.com   →  demo store (static) + API proxy → 127.0.0.1:7700
#   a.vectoriasearch.com →  Algolia-compatible adapter → 127.0.0.1:8108

# ── HTTP → HTTPS redirect for all vectoriasearch.com domains ───────────────
server {
    listen 80;
    listen [::]:80;
    server_name vectoriasearch.com www.vectoriasearch.com
                demo.vectoriasearch.com
                a.vectoriasearch.com;

    location /.well-known/acme-challenge/ {
        root /var/www/html;
    }

    location / {
        return 301 https://$host$request_uri;
    }
}

# ── www → apex redirect ─────────────────────────────────────────────────────
server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name www.vectoriasearch.com;

    ssl_certificate     /etc/letsencrypt/live/vectoriasearch.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/vectoriasearch.com/privkey.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_prefer_server_ciphers off;
    ssl_session_cache shared:VecSSL:10m;
    ssl_session_tickets off;

    access_log /var/log/nginx/vectoriasearch.com-access.log combined;

    return 301 https://vectoriasearch.com$request_uri;
}

# ── vectoriasearch.com — marketing website ─────────────────────────────────
server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name vectoriasearch.com;

    ssl_certificate     /etc/letsencrypt/live/vectoriasearch.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/vectoriasearch.com/privkey.pem;
    ssl_trusted_certificate /etc/letsencrypt/live/vectoriasearch.com/chain.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers 'ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384';
    ssl_prefer_server_ciphers off;
    ssl_session_timeout 1d;
    ssl_session_cache shared:VecSSL:10m;
    ssl_session_tickets off;
    ssl_stapling on;
    ssl_stapling_verify on;
    resolver 8.8.8.8 8.8.4.4 valid=300s;

    root /opt/apps/vectoria/website;
    index index.html;

    access_log /var/log/nginx/vectoriasearch.com-access.log combined;
    error_log  /var/log/nginx/vectoriasearch.com-error.log warn;

    add_header Strict-Transport-Security "max-age=31536000; includeSubDomains; preload" always;
    add_header X-Frame-Options "SAMEORIGIN" always;
    add_header X-Content-Type-Options "nosniff" always;
    add_header Referrer-Policy "strict-origin-when-cross-origin" always;

    location ~* \.(css|js|woff2?|png|jpg|jpeg|svg|ico)$ {
        expires 7d;
        add_header Cache-Control "public, immutable";
    }

    location / {
        try_files $uri $uri/ $uri.html =404;
    }

    location /nginx-health {
        access_log off;
        return 200 "healthy\n";
        add_header Content-Type text/plain;
    }
}

# ── demo.vectoriasearch.com — demo store + Vectoria API proxy ──────────────
upstream vectoria_api {
    server 127.0.0.1:7700 fail_timeout=10s max_fails=3;
    keepalive 16;
}

server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name demo.vectoriasearch.com;

    ssl_certificate     /etc/letsencrypt/live/vectoriasearch.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/vectoriasearch.com/privkey.pem;
    ssl_trusted_certificate /etc/letsencrypt/live/vectoriasearch.com/chain.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers 'ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384';
    ssl_prefer_server_ciphers off;
    ssl_session_timeout 1d;
    ssl_session_cache shared:VecSSL:10m;
    ssl_session_tickets off;
    ssl_stapling on;
    ssl_stapling_verify on;
    resolver 8.8.8.8 8.8.4.4 valid=300s;

    root /opt/apps/vectoria/webstore;
    index index.html;

    access_log /var/log/nginx/demo.vectoriasearch.com-access.log combined;
    error_log  /var/log/nginx/demo.vectoriasearch.com-error.log warn;

    add_header Strict-Transport-Security "max-age=31536000; includeSubDomains; preload" always;
    add_header X-Frame-Options "SAMEORIGIN" always;
    add_header X-Content-Type-Options "nosniff" always;
    add_header Referrer-Policy "strict-origin-when-cross-origin" always;

    # Vectoria API — proxy to vectoria-server container
    location ~ ^/(search|products|events|stats|health|indexes)(/.*)?$ {
        proxy_pass http://vectoria_api;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_set_header Connection "";
        proxy_connect_timeout 30s;
        proxy_send_timeout    60s;
        proxy_read_timeout    60s;
    }

    location / {
        try_files $uri $uri/ =404;
    }
}

# ── a.vectoriasearch.com — Algolia-compatible adapter ────────────────
upstream vectoria_algolia {
    server 127.0.0.1:8108 fail_timeout=10s max_fails=3;
    keepalive 16;
}

server {
    listen 443 ssl http2;
    listen [::]:443 ssl http2;
    server_name a.vectoriasearch.com;

    ssl_certificate     /etc/letsencrypt/live/vectoriasearch.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/vectoriasearch.com/privkey.pem;
    ssl_trusted_certificate /etc/letsencrypt/live/vectoriasearch.com/chain.pem;
    ssl_protocols TLSv1.2 TLSv1.3;
    ssl_ciphers 'ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384';
    ssl_prefer_server_ciphers off;
    ssl_session_timeout 1d;
    ssl_session_cache shared:VecSSL:10m;
    ssl_session_tickets off;
    ssl_stapling on;
    ssl_stapling_verify on;
    resolver 8.8.8.8 8.8.4.4 valid=300s;

    access_log /var/log/nginx/a.vectoriasearch.com-access.log combined;
    error_log  /var/log/nginx/a.vectoriasearch.com-error.log warn;

    add_header Strict-Transport-Security "max-age=31536000; includeSubDomains; preload" always;
    add_header X-Frame-Options "SAMEORIGIN" always;
    add_header X-Content-Type-Options "nosniff" always;

    client_max_body_size 16m;

    location / {
        proxy_pass http://vectoria_algolia;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $http_upgrade;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
        proxy_connect_timeout 30s;
        proxy_send_timeout    60s;
        proxy_read_timeout    60s;
    }
}
