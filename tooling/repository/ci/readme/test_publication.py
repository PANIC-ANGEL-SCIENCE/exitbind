"""Exercise publication only against a disposable local bare repository."""
import pathlib, subprocess, tempfile, json
import unittest


class Publication(unittest.TestCase):
    def test_local_publication_boundary(self):
        source=pathlib.Path(__file__).resolve().parents[4]
        with tempfile.TemporaryDirectory() as tmp:
         p=pathlib.Path(tmp); remote=p/'remote.git'; repo=p/'repo'
         def run(*cmd,cwd=repo,ok=True):
          x=subprocess.run(cmd,cwd=cwd,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
          if ok and x.returncode: raise AssertionError(x.stdout)
          return x
         run('git','init','--bare',str(remote),cwd=p);run('git','init','-b','main',str(repo),cwd=p)
         run('git','config','user.name','Reviewer');run('git','config','user.email','review@example.invalid')
         (repo/'tooling/repository/ci/readme').mkdir(parents=True)
         (repo/'tooling/repository/ci/readme/publish.sh').write_text((source/'tooling/repository/ci/readme/publish.sh').read_text())
         (repo/'README.md').write_text('Test main')
         stub='''import sys,pathlib,json,subprocess\no=pathlib.Path(sys.argv[2]);o.mkdir(exist_ok=True)\nfor n in ['ci.svg','release.svg','license.svg']: (o/n).write_text('<svg/>')\nsha=subprocess.check_output(['git','rev-parse','HEAD']).decode().strip()\n(o/'status.json').write_text(json.dumps({'main_sha':sha}))\n'''
         (repo/'tooling/repository/ci/readme/badges.py').write_text(stub)
         run('git','add','.');run('git','commit','-m','base');run('git','remote','add','origin',str(remote));run('git','push','origin','main')
         before=run('git','rev-parse','HEAD').stdout.strip()
         run('sh','tooling/repository/ci/readme/publish.sh')
         names=run('git','--git-dir='+str(remote),'ls-tree','--name-only','readme-badges').stdout.splitlines()
         assert sorted(names)==['ci.svg','license.svg','release.svg','status.json'],names
         first=run('git','--git-dir='+str(remote),'rev-parse','readme-badges').stdout.strip()
         run('sh','tooling/repository/ci/readme/publish.sh')
         assert first==run('git','--git-dir='+str(remote),'rev-parse','readme-badges').stdout.strip()
         assert before==run('git','rev-parse','HEAD').stdout.strip()
         # Existing branch update is a fast-forward and retains only four files.
         (repo/'tooling/repository/ci/readme/badges.py').write_text(stub.replace("'<svg/>'", "'<svg>updated</svg>'"))
         run('sh','tooling/repository/ci/readme/publish.sh')
         second=run('git','--git-dir='+str(remote),'rev-parse','readme-badges').stdout.strip()
         assert second!=first
         run('git','--git-dir='+str(remote),'merge-base','--is-ancestor',first,second)
         # Force a stale main snapshot; publication must leave remote branch intact.
         (repo/'tooling/repository/ci/readme/badges.py').write_text(stub.replace("{'main_sha':sha}", "{'main_sha':'0'*40}"))
         fail=run('sh','tooling/repository/ci/readme/publish.sh',ok=False)
         assert fail.returncode and 'Main advanced' in fail.stdout,fail.stdout
         assert second==run('git','--git-dir='+str(remote),'rev-parse','readme-badges').stdout.strip()
         print('PASS local bare-remote: orphan bootstrap exactly four files, idempotent identical data, fast-forward update, unchanged main, stale-main publication refused')

         # Deliberately malformed local badge branch must be refused before copying.
         (repo/'tooling/repository/ci/readme/badges.py').write_text(stub)
         edit=p/'edit';run('git','clone','--branch','readme-badges',str(remote),str(edit),cwd=p)
         run('git','config','user.name','Reviewer',cwd=edit);run('git','config','user.email','review@example.invalid',cwd=edit)
         for mode in ('symlink','executable','tree'):
          run('git','reset','--hard',second,cwd=edit)
          f=edit/'ci.svg'
          if mode=='symlink':
           f.unlink();f.symlink_to(str(p/'must-not-be-overwritten'));(p/'must-not-be-overwritten').write_text('safe')
          elif mode=='executable': f.chmod(0o755)
          else: f.unlink();f.mkdir();(f/'nested').write_text('nested')
          run('git','add','.',cwd=edit);run('git','commit','-m',mode,cwd=edit)
          # Local fixture setup only; production script is separately verified nonforce.
          run('git','push','--force','origin','HEAD:readme-badges',cwd=edit)
          fail=run('sh','tooling/repository/ci/readme/publish.sh',ok=False)
          assert fail.returncode and 'Unexpected badge branch files' in fail.stdout,fail.stdout
          if mode=='symlink': assert (p/'must-not-be-overwritten').read_text()=='safe'
         print('PASS malformed local badge branch: symlink, executable and tree entries refused; symlink target unchanged')
