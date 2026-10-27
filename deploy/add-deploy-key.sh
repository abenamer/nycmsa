#!/usr/bin/env bash
# One-time setup on the server: let GitHub Actions run update.sh after
# each merge to main. Creates a `deploy` user whose only permitted
# action over SSH is `sudo /opt/nycmsa/update.sh`, and prints the two
# values to store as GitHub secrets.
#
#   /opt/nycmsa/add-deploy-key.sh 'ssh-ed25519 AAAA... github-actions-deploy'
#
# The public key comes from a key pair made for this purpose on your own
# machine (`ssh-keygen -t ed25519 -f nycmsa-deploy -N ""`); the private
# half becomes the DEPLOY_SSH_KEY secret and is never copied here.
set -euo pipefail

main() {
  local key=${1:?usage: add-deploy-key.sh '<public key>'}
  case "$key" in
    *$'\n'*|*$'\r'*) echo "the key must be a single line" >&2; return 2 ;;
    ssh-ed25519\ *|ecdsa-sha2-*\ *|ssh-rsa\ *) ;;
    *) echo "that does not look like an SSH public key" >&2; return 2 ;;
  esac
  ssh-keygen -lf /dev/stdin <<<"$key" >/dev/null || { echo "ssh-keygen cannot read that key" >&2; return 2; }
  if ! id -u deploy >/dev/null 2>&1; then
    useradd --system --create-home --home-dir /home/deploy --shell /bin/sh deploy
  fi
  # useradd leaves the password field as "!", which sshd reads as a locked
  # account and refuses even with a key. "*" means no password at all.
  usermod -p '*' deploy
  install -d -m 0700 -o deploy -g deploy /home/deploy/.ssh
  # `restrict` turns off port forwarding, agent forwarding, X11 and the
  # pty; `command=` ignores whatever the client asks to run.
  printf 'restrict,command="sudo -n /opt/nycmsa/update.sh" %s\n' "$key" \
    > /home/deploy/.ssh/authorized_keys
  chown deploy:deploy /home/deploy/.ssh/authorized_keys
  chmod 0600 /home/deploy/.ssh/authorized_keys
  # The trailing "" allows the script only without arguments.
  printf 'deploy ALL=(root) NOPASSWD: /opt/nycmsa/update.sh ""\n' > /etc/sudoers.d/nycmsa-deploy
  chmod 0440 /etc/sudoers.d/nycmsa-deploy
  visudo -cf /etc/sudoers.d/nycmsa-deploy >/dev/null
  local ip
  ip=$(hostname -I | awk '{print $1}')
  echo "Done. In the repository's production environment add these secrets:"
  echo
  echo "DEPLOY_HOST:"
  echo "$ip"
  echo
  echo "DEPLOY_KNOWN_HOSTS:"
  echo "$ip $(cut -d' ' -f1,2 /etc/ssh/ssh_host_ed25519_key.pub)"
  echo
  echo "DEPLOY_SSH_KEY: the private key file made with ssh-keygen, whole contents."
}

main "$@"
