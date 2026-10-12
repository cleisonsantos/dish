#!/usr/bin/env bash
# Roda testes de UI em Xvfb com uma nova tentativa limitada.
#
# Os testes dependem de OCR e de tempo de pintura num servidor sem GPU: um
# frame perdido não deve reprovar uma mudança. Uma falha determinística falha
# nas duas tentativas, e aí o CI quebra de verdade.
set -u

status=0
for test in "$@"; do
    if ! xvfb-run -a -s '-screen 0 1280x960x24' python3 "$test"; then
        echo "::warning::$test falhou; tentando mais uma vez"
        sleep 5
        if ! xvfb-run -a -s '-screen 0 1280x960x24' python3 "$test"; then
            echo "::error::$test falhou duas vezes"
            status=1
        fi
    fi
done
exit "$status"
