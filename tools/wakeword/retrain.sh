#!/usr/bin/env bash
# Réentraîne seulement le modèle principal, avec les extraits du mode apprentissage (voir README, « 3 bis »).
MODELS=dis_iris FRESH=1 GRID=${GRID:-128:1000:300,128:800:250,128:600:200} exec bash "$(dirname "$0")/train_best.sh"
