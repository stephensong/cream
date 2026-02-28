GUARDIAN_S9PK := cream-guardian.s9pk
DOC_IMAGE := cream-guardian

.PHONY: all clean docker s9pk verify

all: s9pk

# Build the Docker image from the repo root (needs full workspace context)
docker:
	docker buildx build \
		--file cream-startos/Dockerfile \
		--tag $(DOC_IMAGE) \
		..

# Pack the .s9pk using start-sdk
s9pk: docker
	start-sdk pack

# Verify the built package
verify: s9pk
	start-sdk verify s9pk $(GUARDIAN_S9PK)

clean:
	rm -f $(GUARDIAN_S9PK)
