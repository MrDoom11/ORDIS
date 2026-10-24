"""Caminho das métricas do contador local; nenhuma credencial é necessária."""
import json
from pathlib import Path


def metric_outputs(project=None):
    project = Path(project) if project is not None else Path(__file__).resolve().parents[1]
    # Caminho relativo (o padrão, "runtime") é resolvido a partir do projeto.
    runtime = project / json.loads((project / "config/paths.json").read_text(encoding="utf-8-sig"))["runtime"]
    # O app ORDIS lê este arquivo para mostrar tokens/s.
    return [runtime / "token-metrics.json"]
