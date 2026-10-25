#!/usr/bin/env bash
# Roda cada smoke uma única vez em Xvfb.
#
# O retry antigo escondia flakiness e dobrava o tempo de um teste que falhava.
# O determinismo agora vem das observações de paint e do próprio teste; uma
# falha aqui é uma falha real e deve aparecer no primeiro frame.
set -u

status=0
for test in "$@"; do
    echo "::group::$test"
    if ! xvfb-run -a -s '-screen 0 1280x960x24' python3 "$test"; then
        echo "::error::$test falhou"
        status=1
    fi
    echo "::endgroup::"
done
exit "$status"
