# Docker-Stack-Deploy

This repo is the home of a small but powerful utility that helps you to do
gitops and maintain your docker deployments by checking in the corresponding
configuration files into a private git repo and then running `git push` for
them to take effect.

## Architecture

There are a couple of pieces:

* A private git repo that holds your infrastructure definition,
  essentially a set of directories with docker `compose.yml` files,
  and an encrypted keepass database file that holds any secrets
  that might be required by those containers.  Each of these directories
  if referred to as a "stack".
* One or more hosts on which you are running docker
* The `docker-stack-deploy` container that runs persistently
  on each of those docker hosts

## Getting Started

### Infrastructure Repo

If you don't already have a repo suitable for this purpose, create a new
private one on GitHub.

`docker-stack-deploy` is fairly un-opinionated about directory layout, but
for the sake of getting started:

 * You will need to create a KeePass database to hold your encrypted secrets.
   On macOS you might want to look at [Strongbox](https://strongboxsafe.com),
   which can be used for free for this purpose, but you can also use
   [KeePassXC](https://keepassxc.org) for free on macOS or any other OS.

    * Clone your infra repo locally
    * Create a new vault/database in the root of your infra repo, and name
      it `.secrets.kdbx`.  If you have the option to select the file format
      version, select `Keepass password database 2.x KDBX` in order to
      be compatible with the [rust keepass crate](https://docs.rs/keepass/latest/keepass/)
    * Use a passphrase, rather than a key file, when you configure this database.
    * You will need that passphrase when you edit the database later, and
      also to bootstrap a docker host.

 * You can now create a directory for each of your stack(s).  They can be
   anywhere in the repo; they will be located based on the `stack-deploy.toml`
   file.  I personally have some stacks deployed under `hosts/HOSTNAME/STACKNAME`
   and some under `services/STACKNAME`.  You can pick whatever organization makes
   sense to you, but each individual stack must live in its own directory.

 * In the stack directory you will need at least two files:
    * `compose.yml` - the docker stack definition
    * `stack-deploy.toml` - the definition for stack-deploy

#### Example Stack

In your infra repo, create a `minecraft` directory, and populate it:

Put this in `minecraft/compose.yml`:

```yml
services:
 minecraft:
   image: itzg/minecraft-server
   ports:
     - "25565:25565"
   environment:
     EULA: "TRUE"
   deploy:
     resources:
       limits:
         memory: 1.5G
   volumes:
     - minecraft_data:/data

volumes:
  minecraft_data:
```

> [!IMPORTANT]
> Avoid using local directories for mutable state, as you want to avoid dirtying
> your infrastructure repo checkout with files created by docker and potentially
> cause permission problems and potentially causing conflicts with future changes
> in your Git repo.  I recommend using docker volumes, such as the `minecraft_data`
> volume in the example above, to hold the mutable state.
> Read-only mounts using config files in your repo are fine, and I used those
> often.

And this in `minecraft/stack-deploy.toml`:

```toml
# The name of the stack. Used for dependency purposes
name = "minecraft"

# Lists the hosts on which this stack should run.
# It should match the hostname of your docker host.
runs_on = ["mydockerhostname"]
```

Add and commit that to your infra repo and push it to github.

## Bootstrapping

You need to deploy `docker-stack-deploy` to your docker host. This is done
via a one-time bootstrap procedure.

You will need:

* The infra repo URL
* A PAT that grants access to read from the infra repo
* The passphrase for your infra keepass secrets db
* Access to the docker host

First, login to the docker host, and run the bootstrap command.
You must run this as a user that has privileges to talk to docker:

```console
$ docker run --rm -it \
    -v /var/run/docker.sock:/var/run/docker.sock \
    -v /var/lib/docker-stack-deploy:/var/lib/docker-stack-deploy \
    ghcr.io/wez/docker-stack-deploy \
    docker-stack-deploy bootstrap \
    --project-dir /var/lib/docker-stack-deploy \
    --git-url https://github.com/YOURNAME/REPO.git
Github Token:
KeePass Passphrase:
```

This will pull the deploy image and run it, and it will then prompt you
for your github token and keepass passphrase.

With that done, you can now see what is happening with the deployment:

```console
$ docker logs docker-stack-deploy --tail 100 --follow
```

after a few moments, it should have pulled and launched the minecraft
container.

`docker-stack-deploy` will pull your infra repo every 5 minutes
to look for changes. If any files have changed, it will run through
and deploy each stack.

The deploy command that gets run for each stack is:

```
docker compose up --detach --wait --remove-orphans
```

with the environment populated as described in the *Secrets* section below.

## Secrets

The standard easy way to manage secrets with docker compose is to put
them into an `.env` file in the stack directory.  While you can
do that here, it isn't ideal to check in clear-text secrets.  This is
where the KeePass database comes into play.

You can record the relevant secrets in this database and it will be
stored encrypted on disk.  With a sufficiently strong passphrase
this is a significant upgrade over clear text `.env` files.

Secrets are selectively exposed to a stack based on the instructions
in your `stack-deploy.toml` file for that stack.

For example, if you have this in `gitea/compose.yml`:

```yml
services:
  gitea:
    image: gitea/gitea:latest
    environment:
      - DB_TYPE=postgres
      - DB_HOST=db:5432
      - DB_NAME=gitea
      - DB_USER=gitea
      - DB_PASSWD=${DB_PASSWD}
    restart: always
    volumes:
      - git_data:/data
    ports:
      - 3000:3000
  db:
    image: postgres:alpine
    environment:
      - POSTGRES_USER=gitea
      - POSTGRES_PASSWORD=gitea
      - POSTGRES_DB=${DB_PASSWD}
    restart: always
    volumes:
      - db_data:/var/lib/postgresql/data
    expose:
      - 5432
volumes:
  db_data:
  git_data:
```

and this in `gitead/stack-deploy.toml`:

```toml
name = "gitea"

[secret_env]
DB_PASSWD = 'Database/Gitea Postgres DB/password'
```

Then create an entry in your secrets DB called `Gitea Postgres DB`, this stack
now securely holds the relevant credential in the secrets database.  At deploy
time only the credentials listed in the `secret_env` section will be decrypted
and set in the environment when `docker compose` is run.

`docker-stack-deploy` doesn't create or modify a `.env` file; those environment
variables are set only in the context of the docker invocation.

## Stack Dependencies

You can express dependencies between stacks on the same host.  For example:

```toml
# This is the homepage stack
name = "homepage"
# It runs on the docker1 host
runs_on = ["docker1"]
# It requires that the traefik stack on docker1 be deployed first
depends_on = ["traefik"]
```

The stacks are topologically sorted based on their dependencies and then
started in that order.

It is not possible to depend on stacks that are running on other hosts.

## Stopping and removing a Stack

This is a two phase process:

* First you must edit the `compose.yml` and add `scale: 0` to each service in
  the compose file, then commit and push that and wait 5 minutes or so for
  the change to take effect.  It tells docker to scale down to 0 and stop
  the service.

* Once the service has stopped on all hosts, you can then `git rm` the stack
  directory, commit and push.

## How do I force deployment to run?

If you don't want to wait 5 minutes for it to happen naturally, you can
ssh into your docker host and run `docker restart docker-stack-deploy`.
That will cause it to pull the repo immediately and run through the
deploy commands.

## How do I deploy without local directory without a git repo?

If you don't want to use git repos, you can run the following command. Set 
`--poll-interval` to 0 to disable the polling.

```console
docker-stack-deploy run --repo-dir=/path/to/your/stacks --poll-interval=0
```

## Troubleshooting

You can use `docker compose ls` to review the stacks that are running.
It might look something like this:

```console
$ docker compose ls
NAME                  STATUS              CONFIG FILES
docker-stack-deploy   running(1)          /var/lib/docker-stack-deploy/compose.yml
dockerproxy           running(1)          /var/lib/docker-stack-deploy/repo/services/dockerproxy/compose.yml
frigate               running(1)          /var/lib/docker-stack-deploy/repo/hosts/huge/frigate/compose.yml
immich                running(4)          /var/lib/docker-stack-deploy/repo/hosts/huge/immich/compose.yml
jellyfin              running(2)          /var/lib/docker-stack-deploy/repo/hosts/huge/jellyfin/compose.yml
```

The `/var/lib/docker-stack-deploy` directory is the location where `docker-stack-deploy`
maintains its state.

In that directory:

* The `compose.yml` file was created from the [compose.yml](compose.yml) file
  present in this repository when the docker-stack-deploy image was build by
  my CI.
* There is a `.env` file that captures the secrets from your bootstrap invocation.
* The `repo` directory is where your infrastructure repo is checked out

### To stop a stack

If I wanted to stop frigate:

```console
$ cd /var/lib/docker-stack-deploy/repo/hosts/huge/frigate/
$ docker compose down
```

To bring it back up again, `docker restart docker-stack-deploy`.

