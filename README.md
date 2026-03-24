# TravelAI

## Important:

Loading secrets from env

```bash
eval "$(./load_env.sh)"
```

Build the vibe coding container:

```bash
docker build \
  --build-arg USER_UID=$(id -u) \
  --build-arg USER_GID=$(id -g) \
  -t vibe-nix-env .
```

Run the vibe coding container:

```bash
docker run -it --rm \
  -v $(pwd):/home/developer/project \
  vibe-nix-env
```
