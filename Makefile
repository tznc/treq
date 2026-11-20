bump:
	@CURRENT=$$(grep -m1 '^version = ' src-tauri/Cargo.toml | sed 's/.*"\(.*\)".*/\1/'); \
	NEW="$(VERSION)"; \
	if [ -z "$$NEW" ]; then \
		printf "Current version: $$CURRENT\nNew version: "; \
		read NEW; \
	fi; \
	if [ -z "$$NEW" ]; then echo "No version entered, aborting."; exit 1; fi; \
	echo "Bumping $$CURRENT -> $$NEW"; \
	perl -0pi -e 's/"version": "[^"]*"/"version": "'"$$NEW"'"/' package.json; \
	perl -0pi -e 's/"version": "[^"]*"/"version": "'"$$NEW"'"/' src-tauri/tauri.conf.json; \
	perl -0pi -e 's/^version = "[^"]*"/version = "'"$$NEW"'"/m' src-tauri/Cargo.toml; \
	perl -0pi -e 's/("name": "treq",\s*)"version": "[^"]*"/$$1"version": "'"$$NEW"'"/g' package-lock.json; \
	(cd src-tauri && cargo update -p treq 2>/dev/null) || true; \
	echo "Done."

start:
	supabase start

stop:
	supabase stop

restart:
	$(MAKE) stop
	$(MAKE) start

db.reset:
	supabase db reset --local

db.diff:
	@NAME="$(NAME)"; \
	if [ -z "$$NAME" ]; then \
		printf "Migration name: "; \
		read NAME; \
	fi; \
	if [ -z "$$NAME" ]; then echo "No migration name entered, aborting."; exit 1; fi; \
	supabase db diff --local --file "$$NAME"


deploy:
	@echo 'Deploying DB migrations now'
	@supabase db push
	@echo 'Deploying functions now'
	@for entrypoint in supabase/functions/*/index.ts; do \
		function_dir=$${entrypoint%/index.ts}; \
		supabase functions deploy "$${function_dir##*/}" || exit $$?; \
	done

# Fat Supabase API image (Alpine, single-tenant) + postgres sidecar

supabase.docker.build:
	docker build -f supabase/docker/Dockerfile -t treq-supabase:local supabase

supabase.docker.up: supabase.docker.build
	@bash supabase/docker/prepare-network.sh
	docker compose -f supabase/docker/docker-compose.yml up -d --build
	@echo "Waiting for health..."
	@for i in $$(seq 1 90); do \
		if curl -sf http://127.0.0.1:54321/health >/dev/null; then \
			echo "treq-supabase ready at http://127.0.0.1:54321"; \
			exit 0; \
		fi; \
		sleep 2; \
	done; \
	echo "treq-supabase failed to become healthy" >&2; \
	docker compose -f supabase/docker/docker-compose.yml logs --tail=80; \
	exit 1

supabase.docker.down:
	docker compose -f supabase/docker/docker-compose.yml down

supabase.docker.smoke:
	bash supabase/docker/smoke.sh

supabase.docker.test:
	@bash supabase/docker/prepare-network.sh
	docker compose -f supabase/docker/docker-compose.test.yml down -v --remove-orphans 2>/dev/null || true
	docker compose -f supabase/docker/docker-compose.test.yml up -d --build --wait
	bash supabase/docker/test/verify-self-hosted.sh
	docker compose -f supabase/docker/docker-compose.test.yml down -v

supabase.docker.logs:
	docker compose -f supabase/docker/docker-compose.yml logs -f

.PHONY: bump start stop restart db.reset db.diff deploy \
	supabase.docker.build supabase.docker.up supabase.docker.down \
	supabase.docker.smoke supabase.docker.test supabase.docker.logs
