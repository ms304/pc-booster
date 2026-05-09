# PC Booster

Un outil Rust pour gérer et optimiser les services Windows en arrêtant les services inutiles et en les maintenant arrêtés.

## Fonctionnalités

- **Lister les services** : Voir tous les services avec leur statut
- **Arrêter/Démarrer des services** : Contrôle manuel des services
- **Monitorer les services** : Surveiller et arrêter automatiquement les services qui redémarrent
- **Blacklist** : Créer une liste de services à arrêter automatiquement
- **Appliquer la blacklist** : Arrêter tous les services de la blacklist en une commande

## Installation

### Prérequis

- Windows 10/11
- Rust et Cargo (installés via `rustup`)
- Visual Studio Build Tools (pour le linker MSVC)

### Compilation

```bash
cd pc-booster
cargo build --release
```

L'exécutable sera disponible dans `target\release\pc-booster.exe`

## Utilisation

### Lister tous les services

```bash
pc-booster.exe list
```

### Lister uniquement les services en cours d'exécution

```bash
pc-booster.exe list --running
```

### Lister uniquement les services arrêtés

```bash
pc-booster.exe list --stopped
```

### Arrêter un service

```bash
pc-booster.exe stop <nom_du_service>
```

Exemple :
```bash
pc-booster.exe stop "Windows Search"
```

### Démarrer un service

```bash
pc-booster.exe start <nom_du_service>
```

### Monitorer des services (les arrête s'ils redémarrent)

```bash
pc-booster.exe monitor --services "service1,service2,service3" --interval 5
```

- `--services` : Liste des services à surveiller (séparés par des virgules)
- `--interval` : Intervalle de vérification en secondes (défaut: 5)

Appuyez sur `Ctrl+C` pour arrêter le monitor.

### Gestion de la blacklist

#### Ajouter un service à la blacklist

```bash
pc-booster.exe blacklist <nom_du_service>
```

#### Retirer un service de la blacklist

```bash
pc-booster.exe unblacklist <nom_du_service>
```

#### Lister les services blacklistés

```bash
pc-booster.exe list-blacklist
```

#### Appliquer la blacklist (arrêter tous les services blacklistés)

```bash
pc-booster.exe apply
```

## Services courants à désactiver

Voici quelques services Windows courants que vous pouvez désactiver pour améliorer les performances :

- `WSearch` - Windows Search (indexation)
- `SysMain` - Superfetch / SysMain (préchargement)
- `DiagTrack` - Telemetry (télémétrie)
- `XblAuthManager` - Xbox Live Auth Manager
- `XblGameSave` - Xbox Live Game Save
- `XboxNetApiSvc` - Xbox Live Networking Service
- `Fax` - Service de télécopie
- `PrintNotify` - Notifications d'impression

## Avertissement

⚠️ **ATTENTION** : Cet outil modifie les services système. Arrêter certains services critiques peut causer des problèmes de stabilité ou de fonctionnalité. Utilisez-le avec prudence et assurez-vous de comprendre ce que fait chaque service avant de l'arrêter.

## Configuration

La configuration (blacklist) est stockée dans :
```
%APPDATA%\pc-booster\config.json
```

## Exemple d'utilisation complet

```bash
# 1. Lister les services en cours d'exécution
pc-booster.exe list --running

# 2. Ajouter des services à la blacklist
pc-booster.exe blacklist WSearch
pc-booster.exe blacklist SysMain
pc-booster.exe blacklist DiagTrack

# 3. Vérifier la blacklist
pc-booster.exe list-blacklist

# 4. Appliquer la blacklist
pc-booster.exe apply

# 5. Monitorer les services pour s'assurer qu'ils restent arrêtés
pc-booster.exe monitor --services "WSearch,SysMain,DiagTrack" --interval 10
```

## Licence

Ce projet est fourni à des fins éducatives. Utilisez-le à vos propres risques.
